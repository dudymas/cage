//! Hooks that are run during cage execution.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use command_runner::{Command, CommandRunner};
#[cfg(test)]
use command_runner::TestCommandRunner;
use errors::*;
#[cfg(test)]
use project::Project;
use util::ToStrOrErr;

/// Keeps track of hook scripts and invokes them at appropriate times.
#[derive(Debug)]
pub struct HookManager {
    /// A directory containing subdirectories for each hook.
    hooks_dir: PathBuf,
}

impl HookManager {
    /// Create a new hook manager that runs hooks from the specified
    /// directory.
    pub fn new<P>(hooks_dir: P) -> Result<HookManager>
        where P: Into<PathBuf>
    {
        Ok(HookManager { hooks_dir: hooks_dir.into() })
    }

    /// Invoke all scripts available for the specified hook, passing
    /// `args` as environment variables.
    pub fn invoke<CR>(&self,
                      runner: &CR,
                      hook_name: &str,
                      env: &BTreeMap<String, String>)
                      -> Result<()>
        where CR: CommandRunner
    {

        let d_dir = self.hooks_dir.join(format!("{}.d", hook_name));
        if !d_dir.exists() {
            // Bail early if we don't have a hooks dir.
            debug!("No hooks for '{}' because {} does not exist",
                   hook_name,
                   &d_dir.display());
            return Ok(());
        }

        let mkerr = || ErrorKind::CouldNotReadDirectory(d_dir.clone());

        // Find all our hook scripts and alphabetize them.
        let mut scripts = vec![];
        for entry in try!(fs::read_dir(&d_dir).chain_err(&mkerr)) {
            let entry = try!(entry.chain_err(&mkerr));
            let path = entry.path();
            trace!("Checking {} to see if it's a hook", path.display());
            let ty = try!(entry.file_type()
                .chain_err(|| ErrorKind::CouldNotReadFile(path.clone())));
            let os_name = entry.file_name();
            let name = try!(os_name.to_str_or_err());
            if ty.is_file() && !name.starts_with('.') && name.ends_with(".hook") {
                trace!("Found hook {}", path.display());
                scripts.push(path)
            }
        }
        scripts.sort();

        // Run all our hook scripts.
        for script in scripts {
            let mut cmd = runner.build(&script);
            for (name, val) in env {
                cmd.env(name, val);
            }
            try!(cmd.exec());
        }

        Ok(())
    }
}

#[test]
fn runs_requested_hook_scripts() {
    use env_logger;
    let _ = env_logger::init();
    let proj = Project::from_example("hello").unwrap();
    let ovr = proj.ovr("development").unwrap();
    let runner = TestCommandRunner::new();
    proj.output(ovr).unwrap();

    proj.hooks().invoke(&runner, "up", &BTreeMap::default()).unwrap();
    assert_ran!(runner, {
        [proj.root_dir().join("config/hooks/up.d/hello.hook")]
    });

    proj.remove_test_output().unwrap();
}