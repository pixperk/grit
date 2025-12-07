mod provider;
mod cli;

use clap::Parser;
use cli::Cli;

fn main() {
    let cli = Cli::parse();

    if cli.verbose {
        println!("Verbose mode enabled");
    }

    println!("{:?}", cli.command);
}

