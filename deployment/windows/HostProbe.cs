namespace Bureau.Windows
{
    using System;
    using System.Collections.Generic;
    using System.Diagnostics;
    using System.IO;
    using System.Runtime.InteropServices;
    using System.Security.Principal;
    using System.Text.RegularExpressions;
    using System.Threading.Tasks;
    using Microsoft.Win32;
    using Microsoft.Win32.SafeHandles;

    public sealed class HostSample
    {
        public string WindowsUserSid, Distro, RegistrationId, RegistrationStamp, BasePath, VhdFileName;
        public string FinalPath, VolumePath, VolumeSerial, FileId;
        public int Version;
        public uint Attributes, Links, FileType, DriveType;
        public bool DeletePending;
        public long PhysicalBytes, LogicalBytes, StartedTicks, FinishedTicks;
        public ulong BackingFreeBytes, MemoryFreeBytes;
    }

    public sealed class HostProbe : IDisposable
    {
        private readonly string distro, registrationId, path;
        private readonly List<(string Path, SafeFileHandle Handle)> handles = new List<(string, SafeFileHandle)>();
        private RegistryKey registration;

        public HostProbe(string distro, string registrationId, string path)
        {
            this.distro = distro; this.registrationId = registrationId; this.path = path;
        }
        public Task<HostSample> SampleAsync() => Task.Run(Capture);
        private HostSample Capture()
        {
            var sample = new HostSample { StartedTicks = Stopwatch.GetTimestamp() };
            ReadRegistration(sample);
            HostNative.Require(String.Equals(Path.Combine(sample.BasePath, sample.VhdFileName), path,
                StringComparison.OrdinalIgnoreCase));
            if (handles.Count == 0) OpenHandles();
            VerifyHandles();
            ReadFile(sample);
            ReadResources(sample);
            // A path or mount change during the counters must not validate the earlier identity.
            VerifyHandles();
            sample.FinishedTicks = Stopwatch.GetTimestamp();
            return sample;
        }
        private void ReadRegistration(HostSample sample)
        {
            using var user = WindowsIdentity.GetCurrent();
            sample.WindowsUserSid = user.User?.Value;
            using var root = RegistryKey.OpenBaseKey(RegistryHive.CurrentUser, RegistryView.Registry64);
            using var lxss = root.OpenSubKey(@"Software\Microsoft\Windows\CurrentVersion\Lxss", false);
            HostNative.Require(lxss != null);
            VerifySelection(lxss);
            if (registration == null) registration = lxss.OpenSubKey(registrationId, false);
            HostNative.Require(registration != null);
            sample.Distro = Text(registration, "DistributionName");
            sample.RegistrationId = registrationId;
            sample.BasePath = NormalPath(Text(registration, "BasePath").TrimEnd('\\'));
            sample.VhdFileName = FileName(registration);
            object version = registration.GetValue("Version", null, RegistryValueOptions.DoNotExpandEnvironmentNames);
            HostNative.Require(version is int && registration.GetValueKind("Version") == RegistryValueKind.DWord);
            sample.Version = (int)version;
            sample.RegistrationStamp = RegistrationStamp(registration);
        }
        private void VerifySelection(RegistryKey lxss)
        {
            string[] names = lxss.GetSubKeyNames();
            HostNative.Require(names.Length <= 256);
            int matches = 0;
            foreach (string name in names)
            {
                using var key = lxss.OpenSubKey(name, false);
                HostNative.Require(key != null);
                if (!String.Equals(Text(key, "DistributionName"), distro, StringComparison.Ordinal)) continue;
                HostNative.Require(String.Equals(name, registrationId, StringComparison.OrdinalIgnoreCase));
                matches++;
            }
            HostNative.Require(matches == 1);
        }
        private static string Text(RegistryKey key, string name)
        {
            object value = key.GetValue(name, null, RegistryValueOptions.DoNotExpandEnvironmentNames);
            HostNative.Require(value is string && key.GetValueKind(name) == RegistryValueKind.String);
            HostNative.Require(!String.IsNullOrWhiteSpace((string)value));
            return (string)value;
        }
        private static string FileName(RegistryKey key)
        {
            object value = key.GetValue("VhdFileName", null, RegistryValueOptions.DoNotExpandEnvironmentNames);
            string name = value == null ? "ext4.vhdx" : Text(key, "VhdFileName");
            HostNative.Require(Regex.IsMatch(name, @"\A[^\\/:*?""<>|\p{Cc}]+(?<![. ])\z"));
            return name;
        }
        private static string RegistrationStamp(RegistryKey key)
        {
            int result = HostNative.RegQueryInfoKeyW(key.Handle, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero,
                IntPtr.Zero, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero, out long stamp);
            HostNative.Require(result == 0 && stamp > 0);
            return stamp.ToString("x16");
        }
        private static string NormalPath(string value)
        {
            string path = value.StartsWith(@"\\?\", StringComparison.Ordinal) ? value.Substring(4) : value;
            HostNative.Require(Regex.IsMatch(path, @"\A[A-Z]:\\[^\p{Cc}]*\z") && path.Length <= 240 && !path.Contains('/'));
            HostNative.Require(Path.GetFullPath(path) == path);
            foreach (string part in path.Substring(3).Split('\\'))
                HostNative.Require(part.Length > 0 && !Regex.IsMatch(part, @"[<>:""|?*\p{Cc}]|[. ]\z"));
            return path;
        }
        private void OpenHandles()
        {
            NormalPath(path);
            string current = Path.GetPathRoot(path);
            handles.Add((current, HostNative.Open(current)));
            foreach (string part in path.Substring(current.Length).Split('\\'))
            {
                current = Path.Combine(current, part);
                handles.Add((current, HostNative.Open(current)));
            }
        }
        private void VerifyHandles()
        {
            for (int index = 0; index < handles.Count; index++)
            {
                var entry = handles[index];
                VerifyPath(entry.Handle, entry.Path, index != handles.Count - 1);
                using var fresh = HostNative.Open(entry.Path);
                VerifyPath(fresh, entry.Path, index != handles.Count - 1);
                var heldId = HostNative.Id(entry.Handle);
                var freshId = HostNative.Id(fresh);
                HostNative.Require(heldId.Volume == freshId.Volume &&
                    Convert.ToHexString(heldId.Id) == Convert.ToHexString(freshId.Id));
            }
        }
        private static void VerifyPath(SafeFileHandle handle, string path, bool directory)
        {
            HostNative.Require(HostNative.FileAttributes(handle, 9, out var attrs, 8));
            HostNative.Require((attrs.Value & 0x400) == 0 && ((attrs.Value & 0x10) != 0) == directory);
            string final = HostNative.FinalPath(handle, 0);
            HostNative.Require(final.StartsWith(@"\\?\", StringComparison.Ordinal));
            HostNative.Require(String.Equals(final.Substring(4), path, StringComparison.OrdinalIgnoreCase));
        }
        private void ReadFile(HostSample sample)
        {
            var file = handles[handles.Count - 1].Handle;
            var id = HostNative.Id(file);
            HostNative.Require(HostNative.FileStandard(file, 1, out var standard, (uint)Marshal.SizeOf<HostNative.Standard>()));
            HostNative.Require(HostNative.FileCompression(file, 8, out var compression, (uint)Marshal.SizeOf<HostNative.Compression>()));
            HostNative.Require(HostNative.FileAttributes(file, 9, out var attrs, 8));
            HostNative.Require(compression.Physical > 0 && standard.Eof > 0);
            sample.FinalPath = HostNative.FinalPath(file, 0).Substring(4);
            sample.VolumePath = VolumeRoot(HostNative.FinalPath(file, 1));
            sample.VolumeSerial = id.Volume.ToString("x16");
            sample.FileId = Convert.ToHexString(id.Id).ToLowerInvariant();
            sample.Attributes = attrs.Value; sample.Links = standard.Links; sample.DeletePending = standard.Deleted;
            sample.PhysicalBytes = compression.Physical; sample.LogicalBytes = standard.Eof;
            sample.FileType = HostNative.GetFileType(file);
            sample.DriveType = HostNative.GetDriveTypeW(sample.VolumePath);
        }
        private static string VolumeRoot(string path)
        {
            var match = Regex.Match(path, @"\A\\\\\?\\Volume\{([0-9a-fA-F-]{36})\}\\[^\p{Cc}]*\z");
            HostNative.Require(match.Success && Guid.TryParse(match.Groups[1].Value, out _));
            return @"\\?\Volume{" + Guid.Parse(match.Groups[1].Value).ToString("D") + @"}\";
        }
        private static void ReadResources(HostSample sample)
        {
            HostNative.Require(HostNative.GetDiskFreeSpaceExW(sample.VolumePath, out ulong available,
                out ulong total, out ulong free));
            HostNative.Require(total > 0 && available > 0 && free > 0 && available <= total && free <= total);
            sample.BackingFreeBytes = Math.Min(available, free);
            var memory = new HostNative.Memory { Length = (uint)Marshal.SizeOf<HostNative.Memory>() };
            HostNative.Require(HostNative.GlobalMemoryStatusEx(ref memory));
            HostNative.Require(memory.Total > 0 && memory.Available > 0 && memory.Available <= memory.Total);
            sample.MemoryFreeBytes = memory.Available;
        }
        public void Dispose()
        {
            foreach (var entry in handles) entry.Handle.Dispose();
            registration?.Dispose();
        }
    }
}
