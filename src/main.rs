mod cli;
mod cmd;
mod config;
mod date;
mod error;
mod lang;
mod launch;
mod store;
mod table;
mod ui;

use std::process::ExitCode;

use cli::Cmd;
use error::Result;
use store::Store;

fn main() -> ExitCode {
    let mut args = std::env::args_os();
    let argv0 = args.next().unwrap_or_default();

    // Started as a script's name (through its symlink)? Then run the script.
    if let Some((store, name)) = launch::detect(&argv0) {
        return launch::run(&store, &name, args.collect());
    }

    // Lossy on purpose: a non-UTF-8 name becomes U+FFFD, which fails name validation with a clear message.
    let args: Vec<String> = args.map(|a| a.to_string_lossy().into_owned()).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            e.report();
            ExitCode::from(e.code())
        }
    }
}

fn run(args: &[String]) -> Result<()> {
    match cli::parse(args)? {
        Cmd::Help => print!("{}", cli::help()),
        Cmd::Version => println!("fun {}", env!("CARGO_PKG_VERSION")),
        Cmd::Edit { name, lang } => cmd::edit(&Store::from_env()?, &name, lang.as_deref())?,
        Cmd::Save { name } => cmd::save(&Store::from_env()?, &name)?,
        Cmd::List => cmd::list(&Store::from_env()?)?,
        Cmd::Show { name } => cmd::show(&Store::from_env()?, &name)?,
        Cmd::Rm { names, force } => cmd::rm(&Store::from_env()?, &names, force)?,
    }
    Ok(())
}
