//! Our main CLI tool.

#![cfg_attr(feature="clippy", feature(plugin))]
#![cfg_attr(feature="clippy", plugin(clippy))]

#![deny(warnings)]

#[macro_use]
extern crate cage;
#[macro_use]
extern crate clap;
extern crate env_logger;
#[macro_use]
extern crate log;
extern crate rustc_serialize;
extern crate yaml_rust;

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process;
use yaml_rust::yaml;

use cage::command_runner::{Command, CommandRunner, OsCommandRunner};
use cage::cmd::*;
use cage::Result;

/// Load our command-line interface definitions from an external `clap`
/// YAML file.  We could create these using code, but at the cost of more
/// verbosity.
fn cli(yaml: &yaml::Yaml) -> clap::App {
    clap::App::from_yaml(yaml).version(crate_version!())
}

/// Custom methods we want to add to `clap::App`.
trait ArgMatchesExt {
    /// Do we need to generate `.cage/pods`?  This will probably be
    /// refactored in the future.
    fn should_output_project(&self) -> bool;

    /// Get either the specified target name, or a reasonable default.
    fn target_name(&self) -> &str;

    /// Determine what pods or services we're supposed to act on.
    fn to_acts_on(&self, arg_name: &str) -> cage::args::ActOn;

    /// Extract options shared by `exec` and `run` from our command-line
    /// arguments.
    fn to_process_options(&self) -> cage::args::opts::Process;

    /// Extract `exec` options from our command-line arguments.
    fn to_exec_options(&self) -> cage::args::opts::Exec;

    /// Extract `run` options from our command-line arguments.
    fn to_run_options(&self) -> cage::args::opts::Run;

    /// Extract `exec::Command` from our command-line arguments.
    fn to_exec_command(&self) -> Option<cage::args::Command>;

    /// Extract 'logs' options from our command-line arguments.
    fn to_logs_options(&self) -> cage::args::opts::Logs;
}

impl<'a> ArgMatchesExt for clap::ArgMatches<'a> {
    fn should_output_project(&self) -> bool {
        self.subcommand_name() != Some("export")
    }

    fn target_name(&self) -> &str {
        self.value_of("target")
            .unwrap_or_else(|| {
                if self.subcommand_name() == Some("test") {
                    "test"
                } else {
                    "development"
                }
            })
    }

    fn to_acts_on(&self, arg_name: &str) -> cage::args::ActOn {
        let names: Vec<String> = self.values_of(arg_name)
            .map_or_else(|| vec![], |p| p.collect())
            .iter()
            .map(|p| p.to_string())
            .collect();
        if names.is_empty() {
            cage::args::ActOn::All
        } else {
            cage::args::ActOn::Named(names)
        }
    }

    fn to_process_options(&self) -> cage::args::opts::Process {
        let mut opts = cage::args::opts::Process::default();
        opts.detached = self.is_present("detached");
        opts.user = self.value_of("user").map(|v| v.to_owned());
        opts.allocate_tty = !self.is_present("no-allocate-tty");
        opts
    }

    fn to_exec_options(&self) -> cage::args::opts::Exec {
        let mut opts = cage::args::opts::Exec::default();
        opts.process = self.to_process_options();
        opts.privileged = self.is_present("privileged");
        opts
    }

    fn to_run_options(&self) -> cage::args::opts::Run {
        let mut opts = cage::args::opts::Run::default();
        opts.process = self.to_process_options();
        opts.entrypoint = self.value_of("entrypoint").map(|v| v.to_owned());
        if let Some(environment) = self.values_of("environment") {
            let environment: Vec<&str> = environment.collect();
            for env_val in environment.chunks(2) {
                if env_val.len() != 2 {
                    // Clap should prevent this.
                    panic!("Environment binding '{}' has no value", env_val[0]);
                }
                opts.environment.insert(env_val[0].to_owned(), env_val[1].to_owned());
            }
        }
        opts
    }

    fn to_logs_options(&self) -> cage::args::opts::Logs {
        let mut opts = cage::args::opts::Logs::default();
        opts.follow = self.is_present("follow");
        opts.number = self.value_of("number").map(|v| v.to_owned());
        opts
    }

    fn to_exec_command(&self) -> Option<cage::args::Command> {
        if self.is_present("COMMAND") {
            let values: Vec<&str> = self.values_of("COMMAND").unwrap().collect();
            assert!(values.len() >= 1, "too few values from CLI parser");
            Some(cage::args::Command::new(values[0]).with_args(&values[1..]))
        } else {
            None
        }
    }
}

/// The function which does the real work.  Unlike `main`, we have a return
/// type of `Result` and may therefore use `try!` to handle errors.
fn run(matches: &clap::ArgMatches) -> Result<()> {
    // We know that we always have a subcommand because our `cli.yml`
    // requires this and `clap` is supposed to enforce it.
    let sc_name = matches.subcommand_name().unwrap();
    let sc_matches: &clap::ArgMatches = matches.subcommand_matches(sc_name).unwrap();

    // Handle any subcommands that we can handle without a project
    // directory.
    match sc_name {
        "sysinfo" => {
            try!(all_versions());
            return Ok(());
        }
        "new" => {
            try!(cage::Project::generate_new(&try!(env::current_dir()),
                                             sc_matches.value_of("NAME").unwrap()));
            return Ok(());
        }
        _ => {}
    }

    // Handle our standard arguments that apply to all subcommands.
    let mut proj = try!(cage::Project::from_current_dir());
    if let Some(project_name) = matches.value_of("project-name") {
        proj.set_name(project_name);
    }
    if let Some(default_tags_path) = matches.value_of("default-tags") {
        let f = try!(fs::File::open(default_tags_path));
        let reader = io::BufReader::new(f);
        proj.set_default_tags(try!(cage::DefaultTags::read(reader)));
    }
    try!(proj.set_current_target_name(matches.target_name()));

    // Output our project's `*.yml` files for `docker-compose` if we'll
    // need it.
    if matches.should_output_project() {
        try!(proj.output());
    }

    // Handle our subcommands that require a `Project`.
    let runner = OsCommandRunner::new();
    match sc_name {
        "status" => {
            let acts_on = sc_matches.to_acts_on("POD_OR_SERVICE");
            try!(proj.status(&runner, &acts_on));
        }
        "pull" => {
            let acts_on = sc_matches.to_acts_on("POD_OR_SERVICE");
            try!(proj.pull(&runner, &acts_on));
        }
        "build" => {
            let acts_on = sc_matches.to_acts_on("POD_OR_SERVICE");
            let opts = cage::args::opts::Empty;
            try!(proj.compose(&runner, "build", &acts_on, |_| true, &opts));
        }
        "up" => {
            let acts_on = sc_matches.to_acts_on("POD_OR_SERVICE");
            let opts = cage::args::opts::Up::default();
            try!(proj.up(&runner, &acts_on, &opts));
        }
        "stop" => {
            let acts_on = sc_matches.to_acts_on("POD_OR_SERVICE");
            let opts = cage::args::opts::Empty;
            try!(proj.compose(&runner, "stop", &acts_on, |_| true, &opts));
        }
        "rm" => {
            let acts_on = sc_matches.to_acts_on("POD_OR_SERVICE");
            let opts = cage::args::opts::Empty;
            try!(proj.compose(&runner, "rm", &acts_on, |_| true, &opts));
        }
        "run" => {
            let opts = sc_matches.to_run_options();
            let cmd = sc_matches.to_exec_command();
            let pod = sc_matches.value_of("POD").unwrap();
            try!(proj.run(&runner, pod, cmd.as_ref(), &opts));
        }
        "exec" => {
            let service = sc_matches.value_of("SERVICE").unwrap();
            let opts = sc_matches.to_exec_options();
            let cmd = sc_matches.to_exec_command().unwrap();
            try!(proj.exec(&runner, &service, &cmd, &opts));
        }
        "shell" => {
            let service = sc_matches.value_of("SERVICE").unwrap();
            let opts = sc_matches.to_exec_options();
            try!(proj.shell(&runner, &service, &opts));
        }
        "test" => {
            let service = sc_matches.value_of("SERVICE").unwrap();
            let cmd = sc_matches.to_exec_command();
            try!(proj.test(&runner, &service, cmd.as_ref()));
        }
        "source" => try!(run_source(&runner, &mut proj, sc_matches)),
        "generate" => try!(run_generate(&runner, &proj, sc_matches)),
        "logs" => {
            let acts_on = sc_matches.to_acts_on("POD_OR_SERVICE");
            let opts = sc_matches.to_logs_options();
            try!(proj.logs(&runner, &acts_on, &opts));
        }
        "export" => {
            let dir = sc_matches.value_of("DIR").unwrap();
            try!(proj.export(&Path::new(dir)));
        }
        unknown => unreachable!("Unexpected subcommand '{}'", unknown),
    }

    Ok(())
}

/// Our `source` subcommand.
fn run_source<R>(runner: &R,
                 proj: &mut cage::Project,
                 matches: &clap::ArgMatches)
                 -> Result<()>
    where R: CommandRunner
{
    // We know that we always have a subcommand because our `cli.yml`
    // requires this and `clap` is supposed to enforce it.
    let sc_name = matches.subcommand_name().unwrap();
    let sc_matches: &clap::ArgMatches = matches.subcommand_matches(sc_name).unwrap();

    // Dispatch our subcommand.
    let mut re_output = true;
    match sc_name {
        "ls" => {
            re_output = false;
            try!(proj.source_list(runner));
        }
        "clone" => {
            let alias = sc_matches.value_of("ALIAS").unwrap();
            try!(proj.source_clone(runner, alias));
        }
        "mount" => {
            let alias = sc_matches.value_of("ALIAS").unwrap();
            try!(proj.source_set_mounted(runner, alias, true));
        }
        "unmount" => {
            let alias = sc_matches.value_of("ALIAS").unwrap();
            try!(proj.source_set_mounted(runner, alias, false));
        }
        unknown => unreachable!("Unexpected subcommand '{}'", unknown),
    }

    // Regenerate our output if it might have changed.
    if re_output {
        try!(proj.output());
    }

    Ok(())
}

/// Our `generate` subcommand.
fn run_generate<R>(_runner: &R,
                   proj: &cage::Project,
                   matches: &clap::ArgMatches)
                   -> Result<()>
    where R: CommandRunner
{
    // We know that we always have a subcommand because our `cli.yml`
    // requires this and `clap` is supposed to enforce it.
    let sc_name = matches.subcommand_name().unwrap();
    let sc_matches: &clap::ArgMatches = matches.subcommand_matches(sc_name).unwrap();

    match sc_name {
        // TODO LOW: Allow running this without a project?
        "completion" => {
            let shell = match sc_matches.value_of("SHELL").unwrap() {
                "bash" => clap::Shell::Bash,
                "fish" => clap::Shell::Fish,
                unknown => unreachable!("Unknown shell '{}'", unknown),
            };
            let cli_yaml = load_yaml!("cli.yml");
            cli(cli_yaml).gen_completions("cage", shell, proj.root_dir());
        }
        other => try!(proj.generate(other)),
    }
    Ok(())
}

/// Print the version of this executable.
fn version() {
    println!("cage {}", cage::version());
}

/// Print the version of this executable and also the versions of several
/// tools we use.
fn all_versions() -> Result<()> {
    version();

    let runner = OsCommandRunner::new();
    for tool in &["docker", "docker-compose", "git"] {
        try!(runner.build(tool)
            .arg("--version")
            .exec());
    }
    Ok(())
}

/// Our main entry point.
fn main() {
    // Initialize logging with some custom options, mostly so we can see
    // our own warnings.
    let mut builder = env_logger::LogBuilder::new();
    builder.filter(Some("compose_yml"), log::LogLevelFilter::Warn);
    builder.filter(Some("cage"), log::LogLevelFilter::Warn);
    if let Ok(config) = env::var("RUST_LOG") {
        builder.parse(&config);
    }
    builder.init().unwrap();

    // Parse our command-line arguments.
    let cli_yaml = load_yaml!("cli.yml");
    let matches: clap::ArgMatches = cli(cli_yaml).get_matches();
    debug!("Arguments: {:?}", &matches);

    // Defer all our real work to `run`, and handle any errors.  This is a
    // standard Rust pattern to make error-handling in `main` nicer.
    if let Err(ref err) = run(&matches) {
        // We use `unwrap` here to turn I/O errors into application panics.
        // If we can't print a message to stderr without an I/O error,
        // the situation is hopeless.
        write!(io::stderr(), "Error: ").unwrap();
        for e in err.iter() {
            write!(io::stderr(), "{}\n", e).unwrap();
        }
        if let Some(backtrace) = err.backtrace() {
            write!(io::stderr(), "{:?}\n", backtrace).unwrap();
        }
        process::exit(1);
    }
}
