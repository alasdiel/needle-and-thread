//! Pushes a vault to GitHub over HTTPS, as the app will:
//!
//! ```sh
//! NEEDLE_GITHUB_TOKEN=… cargo run -p needle-vcs --example push -- <vault dir> <https url>
//! ```

use std::{env, path::Path, process::exit, time::Instant};

use needle_vcs::Vault;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let [vault, url] = args.as_slice() else {
        eprintln!("usage: push <vault dir> <https url>");
        exit(2);
    };
    let token = env::var("NEEDLE_GITHUB_TOKEN").ok();
    let vault = Vault::open_or_init(Path::new(vault)).unwrap_or_else(|e| {
        eprintln!("can't open the vault: {e}");
        exit(1);
    });

    let started = Instant::now();
    match vault.push(url, token.as_deref()) {
        Ok(()) => println!("pushed in {:?}", started.elapsed()),
        Err(e) => {
            eprintln!("push failed after {:?}: {e}", started.elapsed());
            exit(1);
        }
    }
}
