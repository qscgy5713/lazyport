use lazyport::custom::{self, MatchKind};
use lazyport::{kill_port, listeners, parse_port, process_info, Advice, Listener};
use std::env;
use std::process::exit;

fn print_table(rows: &[&Listener]) {
    println!("{:<7} {:<8} {:<22} {}", "PORT", "PID", "指令", "位址");
    for l in rows {
        println!("{:<7} {:<8} {:<22} {}", l.port, l.pid, l.command, l.addr);
    }
}

const HELP: &str = "lazyport - 查看 port 佔用並結束進程

用法:
  lazyport              列出所有正在監聽的 TCP port
  lazyport <port>       只看某個 port
  lazyport info <port>  顯示佔用該 port 的程式在做什麼
  lazyport kill <port>  結束佔用該 port 的進程(SIGTERM)
  lazyport kill <port> -f  強制結束(SIGKILL)
  lazyport kill <port> --yes  對「不建議關閉」的程式也照樣結束
  lazyport hint list    列出你的自訂說明
  lazyport hint add <cwd|args|command> <比對內容> <說明> [--advice safe|caution|avoid] [--note 關閉影響]
  lazyport hint rm <id> 刪除自訂說明
  lazyport -h           顯示說明";

fn run_hint(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        None | Some("list") => {
            let rules = custom::list_rules()?;
            if rules.is_empty() {
                println!("還沒有自訂說明(設定檔: {})", custom::config_path()?.display());
            }
            for r in rules {
                println!(
                    "{}  [{}] {} → {} ({})",
                    r.id,
                    r.kind.label(),
                    r.pattern,
                    r.desc,
                    r.advice.label()
                );
            }
            Ok(())
        }
        Some("add") => {
            let usage = "用法: lazyport hint add <cwd|args|command> <比對內容> <說明> [--advice safe|caution|avoid] [--note 關閉影響]";
            let mut positional = Vec::new();
            let (mut advice, mut note) = (Advice::Caution, String::new());
            let mut it = args[1..].iter();
            while let Some(a) = it.next() {
                match a.as_str() {
                    "--advice" => {
                        advice = match it.next().map(String::as_str) {
                            Some("safe") => Advice::Safe,
                            Some("caution") => Advice::Caution,
                            Some("avoid") => Advice::Avoid,
                            _ => return Err("--advice 只能是 safe、caution 或 avoid".into()),
                        }
                    }
                    "--note" => note = it.next().ok_or("--note 後面需要內容")?.clone(),
                    _ => positional.push(a.as_str()),
                }
            }
            let [kind, pattern, desc] = positional[..] else {
                return Err(usage.into());
            };
            let rule = custom::save_rule(None, MatchKind::parse(kind)?, pattern, desc, advice, &note)?;
            println!("已儲存自訂說明 {}:[{}] {} → {}", rule.id, rule.kind.label(), rule.pattern, rule.desc);
            Ok(())
        }
        Some("rm") => {
            let id = args.get(1).ok_or("請指定 id,可用 lazyport hint list 查看")?;
            custom::delete_rule(id)?;
            println!("已刪除 {id}");
            Ok(())
        }
        _ => Err("用法: lazyport hint list | add | rm".into()),
    }
}

fn run(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        None | Some("list") => {
            let all = listeners()?;
            if all.is_empty() {
                println!("目前沒有監聽中的 TCP port");
            } else {
                print_table(&all.iter().collect::<Vec<_>>());
            }
            Ok(())
        }
        Some("-h") | Some("--help") | Some("help") => {
            println!("{HELP}");
            Ok(())
        }
        Some("hint") => run_hint(&args[1..]),
        Some("info") => {
            let port = parse_port(args.get(1).ok_or("請指定 port,例如: lazyport info 3000")?)?;
            let all = listeners()?;
            let rows: Vec<&Listener> = all.iter().filter(|l| l.port == port).collect();
            if rows.is_empty() {
                println!("port {port} 沒有被佔用");
            }
            for l in rows {
                let i = process_info(l.pid)?;
                println!("port {port} ← {} (pid {})", l.command, i.pid);
                let tag = if i.custom_id.is_some() { "(自訂)" } else { "" };
                println!(
                    "  說明:   {}{tag}",
                    i.hint.as_deref().unwrap_or("沒有內建說明,請參考下方路徑與指令判斷")
                );
                println!("  使用者: {}", i.user);
                println!("  啟動:   {}", i.started);
                println!("  執行檔: {}", i.exe);
                println!("  指令列: {}", i.args);
                println!("  工作目錄: {}", i.cwd);
                println!("  建議:   {}", i.advice.label());
                println!("  關閉影響: {}", i.advice_note);
            }
            Ok(())
        }
        Some("kill") => {
            let rest = &args[1..];
            let force = rest.iter().any(|a| a == "-f" || a == "--force");
            let port_arg = rest
                .iter()
                .find(|a| !a.starts_with('-'))
                .ok_or("請指定 port,例如: lazyport kill 3000")?;
            let port = parse_port(port_arg)?;
            let yes = rest.iter().any(|a| a == "-y" || a == "--yes");
            if !yes {
                // 不建議關閉的程式(系統服務、他人進程等)需明確加 --yes 才會結束
                for l in listeners()?.iter().filter(|l| l.port == port) {
                    if let Ok(i) = process_info(l.pid) {
                        if i.advice == Advice::Avoid {
                            return Err(format!(
                                "{} (pid {}) 不建議關閉:{}\n若仍要結束,請加上 --yes",
                                l.command, l.pid, i.advice_note
                            ));
                        }
                    }
                }
            }
            for l in kill_port(port, force)? {
                println!("已結束 {} (pid {})", l.command, l.pid);
            }
            Ok(())
        }
        Some(p) => {
            let port = parse_port(p)?;
            let all = listeners()?;
            let rows: Vec<&Listener> = all.iter().filter(|l| l.port == port).collect();
            if rows.is_empty() {
                println!("port {port} 沒有被佔用");
            } else {
                print_table(&rows);
            }
            Ok(())
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if let Err(e) = run(&args) {
        eprintln!("錯誤: {e}");
        exit(1);
    }
}
