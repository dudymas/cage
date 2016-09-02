//! A conductor project.

use std::env;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::marker::PhantomData;

use dir;
use pod::Pod;
use util::{ConductorPathExt, Error, ToStrOrErr};

/// A `conductor` project, which is represented as a directory containing a
/// `pods` subdirectory.
#[derive(Debug)]
pub struct Project {
    /// The directory which contains our `project`.  Must have a
    /// subdirectory named `pods`.
    pub root_dir: PathBuf,

    /// The directory to which we'll write our transformed pods.  Defaults
    /// to `root_dir.join(".conductor")`.
    pub output_dir: PathBuf,

    /// All the pods associated with this project.
    pub pods: Vec<Pod>,

    /// PRIVATE.  Mark this struct as having unknown fields for future
    /// compatibility.  This prevents direct construction and exhaustive
    /// matching.  This needs to be be public because of
    /// http://stackoverflow.com/q/39277157/12089
    #[doc(hidden)]
    pub _phantom: PhantomData<()>,
}

impl Project {
    /// Create a `Project` using the current directory as input and the
    /// `.conductor` subdirectory as output.
    ///
    /// ```
    /// use conductor::Project;
    /// use std::env;
    ///
    /// let saved = env::current_dir().unwrap();
    /// env::set_current_dir("examples/hello/pods").unwrap();
    ///
    /// let proj = Project::from_current_dir().unwrap();
    /// assert_eq!(proj.root_dir, saved.join("examples/hello"));
    /// assert_eq!(proj.output_dir, saved.join("examples/hello/.conductor"));
    ///
    /// env::set_current_dir(saved).unwrap();
    /// ```
    pub fn from_current_dir() -> Result<Project, Error> {
        // (We can only test this using a doc test because testing it
        // requires messing with `set_current_dir`, which isn't thread safe
        // and will break parallel tests.)
        let current = try!(env::current_dir());
        let root_dir = try!(dir::find_project(&current));
        Ok(Project {
            root_dir: root_dir.clone(),
            output_dir: root_dir.join(".conductor"),
            pods: try!(Project::find_pods(&root_dir)),
            _phantom: PhantomData,
        })
    }

    /// (Tests only.) Create a `Project` from a subirectory of `examples`,
    /// with an output directory under `target/test_output/$NAME`.
    #[cfg(test)]
    pub fn from_example(name: &str) -> Result<Project, Error> {
        let example_dir = Path::new("examples").join(name);
        let root_dir = try!(dir::find_project(&example_dir));
        Ok(Project {
            root_dir: root_dir.clone(),
            output_dir: Path::new("target/test_output").join(name),
            pods: try!(Project::find_pods(&root_dir)),
            _phantom: PhantomData,
        })
    }

    /// Find all the pods defined in this project.
    fn find_pods(root_dir: &Path) -> Result<Vec<Pod>, Error> {
        let mut pods = vec!();
        for glob_result in try!(root_dir.glob("pods/*.yml")) {
            let path = try!(glob_result);
            // It's safe to unwrap the file_stem because we know it matched
            // our glob.
            let name =
                try!(path.file_stem().unwrap().to_str_or_err()).to_owned();
            pods.push(Pod {
                name: name,
                _phantom: PhantomData,
            });
        }
        Ok(pods)
    }
    
    /// Delete our existing output and replace it with a processed and
    /// expanded version of our pod definitions.
    pub fn output(&self) -> Result<(), Error> {
        // Get a path to our input pods directory.
        let in_pods = self.root_dir.join("pods");

        // Get a path to our output pods directory (and delete it if it
        // exists).
        let out_pods = self.output_dir.join("pods");
        if out_pods.exists() {
            try!(fs::remove_dir_all(&out_pods));
        }

        // Iterate over our *.env files recursively.
        for glob_result in try!(in_pods.glob("**/*.env")) {
            let rel = try!(try!(glob_result).strip_prefix(&in_pods)).to_owned();
            let in_path = in_pods.join(&rel);
            let out_path = try!(out_pods.join(&rel).with_guaranteed_parent());
            debug!("Copy {} to {}", in_path.display(), out_path.display());
            try!(fs::copy(in_path, out_path));
        }
        Ok(())
    }
}

#[test]
fn new_from_example_uses_example_and_target() {
    let proj = Project::from_example("hello").unwrap();
    assert_eq!(proj.root_dir, Path::new("examples/hello"));
    assert_eq!(proj.output_dir, Path::new("target/test_output/hello"));
}

#[test]
fn output_copies_env_files() {
    let proj = Project::from_example("hello").unwrap();
    proj.output().unwrap();
    assert!(proj.output_dir.join("pods/common.env").exists());
    assert!(proj.output_dir.join("pods/overrides/test/common.env").exists());
}

#[test]
fn pods_are_loaded() {
    let proj = Project::from_example("hello").unwrap();
    let names: Vec<_> = proj.pods.iter().map(|pod| &pod.name).collect();
    assert_eq!(names, ["frontend"]);
}
