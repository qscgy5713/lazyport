//! 使用者自訂的程式說明:存成 JSON,CLI 與桌面版共用。
//! 自訂說明優先於內建對照表,但不能蓋過安全規則(系統程式、他人進程仍不建議關閉)。
use crate::Advice;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{env, fs};

/// 比對依據
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MatchKind {
    /// 進程的工作目錄(含子目錄),適合辨識自己的專案
    Cwd,
    /// 指令列包含此字串
    Args,
    /// 程式名稱(lsof 的 COMMAND)包含此字串
    Command,
}

impl MatchKind {
    pub fn label(self) -> &'static str {
        match self {
            MatchKind::Cwd => "工作目錄",
            MatchKind::Args => "指令列包含",
            MatchKind::Command => "程式名稱",
        }
    }

    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "cwd" => Ok(MatchKind::Cwd),
            "args" => Ok(MatchKind::Args),
            "command" => Ok(MatchKind::Command),
            _ => Err(format!("無效的比對方式: {s}(可用 cwd / args / command)")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    pub kind: MatchKind,
    pub pattern: String,
    pub desc: String,
    pub advice: Advice,
    #[serde(default)]
    pub note: String,
}

#[derive(Default, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    rules: Vec<Rule>,
}

const MIN_PATTERN: usize = 2;
const MAX_PATTERN: usize = 300;
const MAX_DESC: usize = 200;
const MAX_NOTE: usize = 500;

/// 設定檔路徑:`$LAZYPORT_CONFIG_DIR/hints.json`,未設定則為 `~/.config/lazyport/hints.json`
pub fn config_path() -> Result<PathBuf, String> {
    let dir = match env::var_os("LAZYPORT_CONFIG_DIR") {
        Some(d) => PathBuf::from(d),
        None => PathBuf::from(env::var_os("HOME").ok_or("找不到 HOME 環境變數")?)
            .join(".config")
            .join("lazyport"),
    };
    Ok(dir.join("hints.json"))
}

fn load_from(path: &Path) -> Result<Vec<Rule>, String> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("無法讀取 {}: {e}", path.display())),
    };
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str::<File>(&text)
        .map(|f| f.rules)
        .map_err(|e| format!("{} 格式錯誤: {e}", path.display()))
}

/// 先寫暫存檔再改名,避免寫到一半當機而毀損原檔
fn save_to(path: &Path, rules: &[Rule]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("無法建立設定資料夾: {e}"))?;
    }
    let json = serde_json::to_string_pretty(&File { rules: rules.to_vec() })
        .map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, json).map_err(|e| format!("無法寫入設定: {e}"))?;
    fs::rename(&tmp, path).map_err(|e| format!("無法儲存設定: {e}"))
}

pub fn list_rules() -> Result<Vec<Rule>, String> {
    load_from(&config_path()?)
}

fn new_id() -> String {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    format!("{nanos:x}-{:x}", COUNTER.fetch_add(1, Ordering::Relaxed))
}

/// 驗證並建立規則。`id` 有值代表編輯既有規則。
pub fn build_rule(
    id: Option<String>,
    kind: MatchKind,
    pattern: &str,
    desc: &str,
    advice: Advice,
    note: &str,
) -> Result<Rule, String> {
    let pattern = pattern.trim();
    let (desc, note) = (desc.trim(), note.trim());
    // 太短的比對字串會命中大量不相干的程式
    if pattern.chars().count() < MIN_PATTERN {
        return Err(format!("比對內容至少需要 {MIN_PATTERN} 個字"));
    }
    if pattern.chars().count() > MAX_PATTERN {
        return Err(format!("比對內容最多 {MAX_PATTERN} 個字"));
    }
    if desc.is_empty() {
        return Err("請填寫這是什麼程式".into());
    }
    if desc.chars().count() > MAX_DESC {
        return Err(format!("說明最多 {MAX_DESC} 個字"));
    }
    if note.chars().count() > MAX_NOTE {
        return Err(format!("關閉影響最多 {MAX_NOTE} 個字"));
    }
    if kind == MatchKind::Cwd && !pattern.starts_with('/') {
        return Err("工作目錄必須是絕對路徑(以 / 開頭)".into());
    }
    if kind == MatchKind::Cwd && pattern.trim_end_matches('/').is_empty() {
        return Err("不能用根目錄 / 當比對條件,會命中所有程式".into());
    }
    Ok(Rule {
        id: id.unwrap_or_else(new_id),
        kind,
        pattern: pattern.to_string(),
        desc: desc.to_string(),
        advice,
        note: note.to_string(),
    })
}

/// 新增或更新:id 相同則取代;否則同一個(比對方式, 比對內容)視為同一條,避免重複
pub fn upsert(rules: &mut Vec<Rule>, mut rule: Rule) -> Rule {
    if let Some(i) = rules.iter().position(|r| r.id == rule.id) {
        rules[i] = rule.clone();
    } else if let Some(i) = rules
        .iter()
        .position(|r| r.kind == rule.kind && r.pattern.eq_ignore_ascii_case(&rule.pattern))
    {
        rule.id = rules[i].id.clone();
        rules[i] = rule.clone();
    } else {
        rules.push(rule.clone());
    }
    rule
}

pub fn save_rule(
    id: Option<String>,
    kind: MatchKind,
    pattern: &str,
    desc: &str,
    advice: Advice,
    note: &str,
) -> Result<Rule, String> {
    let rule = build_rule(id, kind, pattern, desc, advice, note)?;
    let path = config_path()?;
    let mut rules = load_from(&path)?;
    let stored = upsert(&mut rules, rule);
    save_to(&path, &rules)?;
    Ok(stored)
}

pub fn delete_rule(id: &str) -> Result<(), String> {
    let path = config_path()?;
    let mut rules = load_from(&path)?;
    let before = rules.len();
    rules.retain(|r| r.id != id);
    if rules.len() == before {
        return Err("找不到這條自訂說明".into());
    }
    save_to(&path, &rules)
}

fn cwd_matches(pattern: &str, cwd: &str) -> bool {
    let p = pattern.trim_end_matches('/');
    cwd == p || cwd.strip_prefix(p).is_some_and(|rest| rest.starts_with('/'))
}

fn rule_matches(rule: &Rule, command: &str, args: &str, cwd: &str) -> bool {
    match rule.kind {
        MatchKind::Cwd => cwd_matches(&rule.pattern, cwd),
        MatchKind::Args => args.to_lowercase().contains(&rule.pattern.to_lowercase()),
        MatchKind::Command => command
            .replace("\\x20", " ")
            .to_lowercase()
            .contains(&rule.pattern.to_lowercase()),
    }
}

/// 多條都命中時,取比對內容最長(最具體)的那條
pub fn find_rule<'a>(rules: &'a [Rule], command: &str, args: &str, cwd: &str) -> Option<&'a Rule> {
    rules
        .iter()
        .filter(|r| rule_matches(r, command, args, cwd))
        .max_by_key(|r| r.pattern.chars().count())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Advice::{Avoid, Caution, Safe};

    fn rule(kind: MatchKind, pattern: &str, desc: &str, advice: Advice) -> Rule {
        build_rule(None, kind, pattern, desc, advice, "").unwrap()
    }

    #[test]
    fn cwd_matches_on_path_boundary() {
        assert!(cwd_matches("/Users/me/ai-trader", "/Users/me/ai-trader"));
        assert!(cwd_matches("/Users/me/ai-trader/", "/Users/me/ai-trader/src"));
        assert!(!cwd_matches("/Users/me/ai", "/Users/me/ai-trader")); // 不可只是字串前綴
        assert!(!cwd_matches("/Users/me/ai-trader", ""));
    }

    #[test]
    fn find_rule_prefers_most_specific() {
        let rules = vec![
            rule(MatchKind::Cwd, "/Users/me/www", "網站資料夾", Caution),
            rule(MatchKind::Cwd, "/Users/me/www/ai-trader", "AI 交易機器人", Avoid),
        ];
        let r = find_rule(&rules, "Python", "python -m app", "/Users/me/www/ai-trader/src").unwrap();
        assert_eq!(r.desc, "AI 交易機器人");
        assert_eq!(find_rule(&rules, "x", "", "/Users/me/www/other").unwrap().desc, "網站資料夾");
        assert!(find_rule(&rules, "x", "", "/tmp").is_none());
    }

    #[test]
    fn args_and_command_match_case_insensitively() {
        let rules = vec![
            rule(MatchKind::Args, "AiTrader", "交易機器人", Safe),
            rule(MatchKind::Command, "my svc", "我的服務", Safe),
        ];
        assert!(find_rule(&rules, "Python", "python -m aitrader.main", "/").is_some());
        assert!(find_rule(&rules, "My\\x20Svc", "", "/").is_some());
    }

    #[test]
    fn validation_rejects_bad_input() {
        let b = |k, p: &str, d: &str| build_rule(None, k, p, d, Safe, "");
        assert!(b(MatchKind::Args, "a", "x").is_err()); // 太短
        assert!(b(MatchKind::Args, "abc", "  ").is_err()); // 沒有說明
        assert!(b(MatchKind::Cwd, "relative/path", "x").is_err()); // 非絕對路徑
        assert!(b(MatchKind::Cwd, "/", "x").is_err()); // 根目錄
        assert!(b(MatchKind::Cwd, "//", "x").is_err());
        assert!(b(MatchKind::Args, &"a".repeat(301), "x").is_err());
        assert!(b(MatchKind::Cwd, "/Users/me/app", "x").is_ok());
    }

    #[test]
    fn upsert_replaces_by_id_or_same_pattern() {
        let mut rules = Vec::new();
        let first = upsert(&mut rules, rule(MatchKind::Args, "foo-bar", "舊說明", Safe));
        // 同一個 (kind, pattern) 再存 → 取代,沿用原 id
        let second = upsert(&mut rules, rule(MatchKind::Args, "FOO-BAR", "新說明", Caution));
        assert_eq!(rules.len(), 1);
        assert_eq!(second.id, first.id);
        assert_eq!(rules[0].desc, "新說明");
        // 以 id 編輯,可改比對內容
        let edited = build_rule(Some(first.id.clone()), MatchKind::Args, "baz-qux", "改名", Safe, "").unwrap();
        upsert(&mut rules, edited);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].pattern, "baz-qux");
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = env::temp_dir().join(format!("lazyport-test-{}", new_id()));
        let path = dir.join("hints.json");
        assert_eq!(load_from(&path).unwrap(), vec![]); // 檔案不存在 → 空
        let rules = vec![rule(MatchKind::Cwd, "/Users/me/app", "我的 App", Avoid)];
        save_to(&path, &rules).unwrap();
        assert_eq!(load_from(&path).unwrap(), rules);
        fs::write(&path, "{ 壞掉的 json").unwrap();
        assert!(load_from(&path).unwrap_err().contains("格式錯誤"));
        let _ = fs::remove_dir_all(dir);
    }
}
