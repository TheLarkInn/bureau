namespace Bureau.Windows
{
    using System;
    using System.Runtime.InteropServices;
    using System.Text;
    using System.Threading;
    using Microsoft.Win32.SafeHandles;

    internal static class HostNative
    {
        [StructLayout(LayoutKind.Sequential)]
        internal struct Identity
        {
            internal ulong Volume;
            [MarshalAs(UnmanagedType.ByValArray, SizeConst = 16)] internal byte[] Id;
        }
        [StructLayout(LayoutKind.Sequential)]
        internal struct Standard
        {
            internal long Allocation, Eof;
            internal uint Links;
            [MarshalAs(UnmanagedType.U1)] internal bool Deleted;
            [MarshalAs(UnmanagedType.U1)] internal bool Directory;
        }
        [StructLayout(LayoutKind.Sequential)]
        internal struct Compression
        {
            internal long Physical;
            internal ushort Format;
            internal byte Unit, Chunk, Cluster;
            [MarshalAs(UnmanagedType.ByValArray, SizeConst = 3)] internal byte[] Reserved;
        }
        [StructLayout(LayoutKind.Sequential)]
        internal struct Attributes { internal uint Value, Tag; }
        [StructLayout(LayoutKind.Sequential)]
        internal struct Memory
        {
            internal uint Length, Load;
            internal ulong Total, Available, TotalPage, AvailablePage, TotalVirtual, AvailableVirtual, Extended;
        }

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        internal static extern SafeFileHandle CreateFileW(string path, uint access, uint share,
            IntPtr security, uint creation, uint flags, IntPtr template);
        [DllImport("kernel32.dll", SetLastError = true, EntryPoint = "GetFileInformationByHandleEx")]
        internal static extern bool FileIdentity(SafeFileHandle file, int type, out Identity value, uint size);
        [DllImport("kernel32.dll", SetLastError = true, EntryPoint = "GetFileInformationByHandleEx")]
        internal static extern bool FileStandard(SafeFileHandle file, int type, out Standard value, uint size);
        [DllImport("kernel32.dll", SetLastError = true, EntryPoint = "GetFileInformationByHandleEx")]
        internal static extern bool FileCompression(SafeFileHandle file, int type, out Compression value, uint size);
        [DllImport("kernel32.dll", SetLastError = true, EntryPoint = "GetFileInformationByHandleEx")]
        internal static extern bool FileAttributes(SafeFileHandle file, int type, out Attributes value, uint size);
        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        internal static extern uint GetFinalPathNameByHandleW(SafeFileHandle file, StringBuilder path, uint size, uint flags);
        [DllImport("kernel32.dll", SetLastError = true)]
        internal static extern uint GetFileType(SafeFileHandle file);
        [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
        internal static extern uint GetDriveTypeW(string root);
        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        internal static extern bool GetDiskFreeSpaceExW(string root, out ulong available, out ulong total, out ulong free);
        [DllImport("kernel32.dll", SetLastError = true)]
        internal static extern bool GlobalMemoryStatusEx(ref Memory status);
        [DllImport("advapi32.dll", CharSet = CharSet.Unicode)]
        internal static extern int RegQueryInfoKeyW(SafeRegistryHandle key, IntPtr name, IntPtr nameLength,
            IntPtr reserved, IntPtr subkeys, IntPtr maxSubkey, IntPtr maxClass, IntPtr values,
            IntPtr maxValueName, IntPtr maxValue, IntPtr security, out long stamp);

        internal static void Require(bool condition)
        {
            if (!condition) throw new InvalidOperationException("windows-resource-query-refused");
        }
        internal static SafeFileHandle Open(string path)
        {
            var handle = CreateFileW(path, 0x80, 3, IntPtr.Zero, 3, 0x02200000, IntPtr.Zero);
            Require(!handle.IsInvalid);
            return handle;
        }
        internal static Identity Id(SafeFileHandle handle)
        {
            Require(FileIdentity(handle, 18, out var value, (uint)Marshal.SizeOf<Identity>()));
            return value;
        }
        internal static string FinalPath(SafeFileHandle handle, uint flags)
        {
            var path = new StringBuilder(512);
            uint length = GetFinalPathNameByHandleW(handle, path, 512, flags);
            Require(length > 0 && length < 512);
            return path.ToString();
        }
    }

    public sealed class Interrupt : IDisposable
    {
        private static int requested;
        public static bool Requested => Volatile.Read(ref requested) != 0;
        public Interrupt() { Volatile.Write(ref requested, 0); Console.CancelKeyPress += Stop; }
        private static void Stop(object sender, ConsoleCancelEventArgs args)
        {
            args.Cancel = true;
            Volatile.Write(ref requested, 1);
        }
        public void Dispose() { Console.CancelKeyPress -= Stop; }
    }
}
