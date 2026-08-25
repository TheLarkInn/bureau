use std::fs::{self, File};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

const ROOT_FILES: [&str; 2] = ["extension.mjs", "serve.mjs"];
const DIRECTORIES: [&str; 2] = ["lib", "web"];
const FIXTURE: &str = "test/fixtures/committed-payload.json";

fn collect_directory(root: &Path, relative: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    let mut entries = fs::read_dir(root.join(relative))?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let nested = relative.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            collect_directory(root, &nested, files)?;
        } else if kind.is_file() {
            files.push(nested);
        }
    }
    Ok(())
}

fn bundle_files(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = ROOT_FILES.map(PathBuf::from).to_vec();
    for directory in DIRECTORIES {
        collect_directory(root, Path::new(directory), &mut files)?;
    }
    files.push(PathBuf::from(FIXTURE));
    Ok(files)
}

fn hash_bytes(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    hash
}

fn bundle_id(root: &Path, files: &[PathBuf]) -> io::Result<String> {
    let mut hash = 14_695_981_039_346_656_037;
    for relative in files {
        hash = hash_bytes(hash, relative.to_string_lossy().as_bytes());
        hash = hash_bytes(hash, &fs::read(root.join(relative))?);
    }
    Ok(format!("{hash:016x}"))
}

fn write_entry(output: &mut File, root: &Path, relative: &Path) -> io::Result<()> {
    let source = fs::canonicalize(root.join(relative))?;
    let name = relative.to_string_lossy().replace('\\', "/");
    writeln!(
        output,
        "    ({name:?}, include_bytes!({:?})),",
        source.to_string_lossy()
    )
}

fn emit_rerun(path: &Path) -> io::Result<()> {
    let output = io::stdout();
    writeln!(output.lock(), "cargo:rerun-if-changed={}", path.display())
}

fn write_manifest(root: &Path, files: &[PathBuf], output: &Path) -> io::Result<()> {
    let mut manifest = File::create(output)?;
    writeln!(
        manifest,
        "pub const BUNDLE_ID: &str = {:?};",
        bundle_id(root, files)?
    )?;
    writeln!(manifest, "pub static FILES: &[(&str, &[u8])] = &[")?;
    for relative in files {
        write_entry(&mut manifest, root, relative)?;
        emit_rerun(&root.join(relative))?;
    }
    writeln!(manifest, "];")
}

fn main() -> io::Result<()> {
    let root = fs::canonicalize(".")?;
    let files = bundle_files(&root)?;
    for directory in DIRECTORIES {
        emit_rerun(&root.join(directory))?;
    }
    let output =
        PathBuf::from(std::env::var_os("OUT_DIR").ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "Cargo did not provide OUT_DIR")
        })?)
        .join("dashboard_assets.rs");
    write_manifest(&root, &files, &output)
}
