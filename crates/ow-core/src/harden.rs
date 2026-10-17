use std::io;

/// Private umask, no core dumps, not dumpable (blocks ptrace and /proc/<pid>/mem access by
/// other same-uid processes). Call first thing in any binary that holds secrets.
pub fn harden_process() -> io::Result<()> {
    use rustix::process::{
        DumpableBehavior, Resource, Rlimit, set_dumpable_behavior, setrlimit, umask,
    };
    umask(rustix::fs::Mode::from_raw_mode(0o077));
    set_dumpable_behavior(DumpableBehavior::NotDumpable)?;
    setrlimit(
        Resource::Core,
        Rlimit {
            current: Some(0),
            maximum: Some(0),
        },
    )?;
    Ok(())
}

/// Create `dir` (0700) if missing, then require it to be a real directory we own with no
/// group/other access. `DirBuilder` modes only apply to directories it creates, so a
/// pre-existing loose directory must be refused, not trusted.
pub fn private_dir(dir: &std::path::Path) -> io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    let md = std::fs::symlink_metadata(dir)?;
    if !md.is_dir() || md.uid() != rustix::process::geteuid().as_raw() || md.mode() & 0o077 != 0 {
        return Err(io::Error::other(format!(
            "{} must be a directory owned by you with mode 0700 (try: chmod 700)",
            dir.display()
        )));
    }
    Ok(())
}
