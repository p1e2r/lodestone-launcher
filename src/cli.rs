//! Headless mode for testing / scripting:
//!   lodestone --cli install <version> [fabric|quilt|forge|neoforge <loader-version>]
//!   lodestone --cli command <version> [...]   (prints the java command)
//!   lodestone --cli loaders <loader> <mc>
//!   lodestone --cli server <name> <version> [loader <lv>]
use anyhow::Result;

use crate::core::instance::{Instance, Kind};
use crate::core::loaders::{self, Loader};
use crate::core::session::Reporter;
use crate::core::{java, launch, net::RT, rules::Env, settings::Settings, versions};

fn parse_loader(s: &str) -> Loader {
    match s {
        "fabric" => Loader::Fabric,
        "quilt" => Loader::Quilt,
        "forge" => Loader::Forge,
        "neoforge" => Loader::NeoForge,
        _ => Loader::Vanilla,
    }
}

pub fn run(args: &[String]) -> Result<()> {
    let mut rep = Reporter::new("cli");
    rep.echo = true;
    RT.block_on(async move {
        match args.first().map(|s| s.as_str()) {
            Some("loaders") => {
                for v in loaders::list_versions(parse_loader(&args[1]), &args[2]).await?.iter().take(10) {
                    println!("{} {}", v.version, if v.stable { "(stable)" } else { "" });
                }
            }
            Some(cmd @ ("install" | "command")) => {
                let mc = &args[1];
                let loader = args.get(2).map(|s| parse_loader(s)).unwrap_or_default();
                let mut inst = Instance::new(&format!("cli-{mc}"), Kind::Client, mc, loader, args.get(3).cloned());
                let id = loaders::install_client(loader, mc, inst.loader_version.as_deref().unwrap_or(""), &rep).await?;
                let res = versions::resolve(&id).await?;
                let mut env = Env::host();
                let plan = java::plan(&res.json, &env).await?;
                env.arch = plan.arch.clone();
                println!("java plan: {plan:?}");
                if let Err(e) = versions::install_client_files(&res, &env, &rep).await {
                    if cmd != "command" {
                        return Err(e);
                    }
                    println!("WARN install incomplete (continuing to print the command): {e:#}");
                }
                let java = java::ensure(&plan, None, &rep).await?;
                if cmd == "command" {
                    let opts = launch::ClientOpts {
                        username: "Tester".into(),
                        game_dir: std::env::temp_dir().join("lodestone-cli-game"),
                        min_mb: 512, max_mb: 2048, extra_jvm: vec![], resolution: None,
                    };
                    let spec = launch::build_client(&res, &java, &env, &opts)?;
                    println!("JAVA {}", spec.java.display());
                    for a in &spec.args { println!("ARG {a}"); }
                }
                println!("OK {id}");
            }
            Some("server") => {
                let loader = args.get(3).map(|s| parse_loader(s)).unwrap_or_default();
                let mut inst = Instance::new(&args[1], Kind::Server, &args[2], loader, args.get(4).cloned());
                inst.server.eula = true;
                inst.save()?;
                crate::core::instance::start_server(inst, Settings::load(), rep.clone()).await?;
            }
            Some("art") => {
                for seed in &args[1..] {
                    println!("{}", crate::ui::art::banner(seed).display());
                }
                println!("{}", crate::ui::art::logo().display());
            }
            Some("account") => {
                let mut accs = crate::core::accounts::Accounts::load();
                let a = accs.add(&args[1])?.clone();
                println!("ADDED {} ({})", a.username, a.offline_uuid());
            }
            _ => println!("usage: lodestone --cli install|command|loaders|server ..."),
        }
        Ok(())
    })
}
