//! lazyport 核心:查詢 TCP 監聽 port 並結束佔用的進程(CLI 與桌面版共用)
pub mod custom;

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::process::Command;

#[derive(Debug, Clone, PartialEq)]
pub struct Listener {
    pub pid: u32,
    pub command: String,
    pub addr: String,
    pub port: u16,
}

/// 解析 `lsof -F pcn` 的輸出:p=pid、c=指令、n=位址(如 *:3000、127.0.0.1:8080)
pub fn parse_lsof(text: &str) -> Vec<Listener> {
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    let mut pid: Option<u32> = None;
    let mut command = String::new();

    for line in text.lines() {
        let (tag, rest) = line.split_at(line.chars().next().map_or(0, |c| c.len_utf8()));
        match tag {
            "p" => {
                pid = rest.parse().ok();
                command.clear();
            }
            "c" => command = rest.to_string(),
            "n" => {
                let port = rest.rsplit(':').next().and_then(|p| p.parse::<u16>().ok());
                if let (Some(pid), Some(port)) = (pid, port) {
                    // IPv4 與 IPv6 會重複列出同一個 port,只留一筆
                    if seen.insert((pid, port)) {
                        result.push(Listener {
                            pid,
                            command: command.clone(),
                            addr: rest.to_string(),
                            port,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    result.sort_by_key(|l| (l.port, l.pid));
    result
}

pub fn listeners() -> Result<Vec<Listener>, String> {
    let out = Command::new("lsof")
        .args(["-nP", "-iTCP", "-sTCP:LISTEN", "-Fpcn"])
        .output()
        .map_err(|e| format!("無法執行 lsof: {e}"))?;
    Ok(parse_lsof(&String::from_utf8_lossy(&out.stdout)))
}

/// 結束佔用指定 port 的所有進程,回傳被結束的進程。
/// 只會對「目前正在監聽該 port」的 pid 發送訊號,不接受任意 pid。
pub fn kill_port(port: u16, force: bool) -> Result<Vec<Listener>, String> {
    let all = listeners()?;
    let targets: Vec<Listener> = all.into_iter().filter(|l| l.port == port).collect();
    if targets.is_empty() {
        return Err(format!("port {port} 沒有被佔用"));
    }
    let signal = if force { "-KILL" } else { "-TERM" };
    for l in &targets {
        let status = Command::new("kill")
            .arg(signal)
            .arg(l.pid.to_string())
            .status()
            .map_err(|e| format!("無法執行 kill: {e}"))?;
        if !status.success() {
            return Err(format!(
                "結束 {} (pid {}) 失敗,可能沒有權限或需要強制結束",
                l.command, l.pid
            ));
        }
    }
    Ok(targets)
}

/// 關閉建議等級
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Advice {
    /// 可以關閉
    Safe,
    /// 可以關閉,但要留意影響
    Caution,
    /// 不建議關閉
    Avoid,
}

impl Advice {
    pub fn as_str(self) -> &'static str {
        match self {
            Advice::Safe => "safe",
            Advice::Caution => "caution",
            Advice::Avoid => "avoid",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Advice::Safe => "可以關閉",
            Advice::Caution => "關閉前請留意",
            Advice::Avoid => "不建議關閉",
        }
    }
}

pub struct Hint {
    pub desc: &'static str,
    pub advice: Advice,
    /// 關閉會有什麼影響、替代做法
    pub note: &'static str,
}

#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: u32,
    pub user: String,
    pub started: String,
    pub exe: String,
    pub args: String,
    pub cwd: String,
    /// 白話說明(內建對照表或使用者自訂)
    pub hint: Option<String>,
    pub advice: Advice,
    pub advice_note: String,
    /// 命中使用者自訂說明時的規則 id
    pub custom_id: Option<String>,
    /// 工作目錄太籠統(空、根目錄、家目錄、系統資料夾),不適合拿來當自訂比對條件
    pub cwd_generic: bool,
}

use Advice::{Avoid, Caution, Safe};

/// 常見程式的白話說明與關閉建議。比對 lsof 的指令名稱或執行檔檔名,先符合者優先,
/// 所以較具體的項目要放在前面。key 以 `=` 開頭表示必須完全相等(避免 `ssh` 誤中 `sshd` 之類)。
const HINTS: &[(&str, Hint)] = &[
    ("launchd", Hint { desc: "macOS 的核心服務管理程式(PID 1)", advice: Avoid, note: "結束它會讓系統不穩定甚至當機,請不要動它" }),
    ("com.docker", Hint { desc: "Docker Desktop 的後端,負責把容器的 port 轉發到本機。port 開著代表有容器在對外提供服務", advice: Caution, note: "結束後所有容器的 port 都會中斷,執行中的資料庫容器可能來不及正常關機。建議從 Docker Desktop 的選單退出,不要直接強制結束" }),
    ("controlcenter", Hint { desc: "macOS 控制中心。5000 / 7000 通常是「AirPlay 接收器」", advice: Avoid, note: "系統程式,結束後 macOS 會自動重啟它,port 也會再被佔用。要釋放 port,請到 系統設定 → 一般 → AirDrop 與接力,關閉「AirPlay 接收器」" }),
    ("rapportd", Hint { desc: "macOS 內建的跨裝置服務(接力、通用剪貼簿等)", advice: Avoid, note: "系統服務,結束後會自動重啟,期間接力與通用剪貼簿可能暫時失效" }),
    ("sharingd", Hint { desc: "macOS 的共享服務(AirDrop、檔案分享)", advice: Avoid, note: "系統服務,結束後會自動重啟,期間 AirDrop 可能暫時失效" }),
    ("identityservicesd", Hint { desc: "macOS 的 iMessage / FaceTime 身分服務", advice: Avoid, note: "系統服務,結束後會自動重啟,期間 iMessage 與 FaceTime 可能暫時失效" }),
    ("mdnsresponder", Hint { desc: "macOS 的 Bonjour 服務,負責區域網路內的裝置探索(印表機、AirPlay 等)", advice: Avoid, note: "系統服務,結束後會自動重啟,期間區域網路的裝置探索可能暫時失效" }),
    ("cupsd", Hint { desc: "macOS 的列印服務(CUPS),631 port 是印表機管理介面", advice: Avoid, note: "系統服務,結束後列印功能會暫時失效,通常會被自動重啟。只想關閉網頁介面,可在終端機執行 cupsctl WebInterface=no" }),
    ("screensharingd", Hint { desc: "macOS 的螢幕共享服務(遠端桌面 / VNC)", advice: Avoid, note: "關閉後無法從遠端看到這台電腦的畫面。要停用請到 系統設定 → 一般 → 共享,關閉「螢幕共享」" }),
    ("smbd", Hint { desc: "macOS 的檔案共享(SMB)服務", advice: Avoid, note: "關閉後其他裝置無法存取你共享的資料夾。要停用請到 系統設定 → 一般 → 共享,關閉「檔案共享」" }),
    ("sshd", Hint { desc: "SSH 遠端登入服務,代表這台電腦開啟了遠端登入", advice: Avoid, note: "關閉後無法再從遠端登入;若你正透過 SSH 連線操作,連線會立即中斷。要停用請到 系統設定 → 一般 → 共享,關閉「遠端登入」" }),
    ("code helper", Hint { desc: "VS Code 的輔助程序(擴充功能、語言伺服器等)", advice: Caution, note: "關閉可能讓擴充功能、終端機或除錯器失效。建議在 VS Code 內執行「重新載入視窗」,不要直接結束" }),
    ("cursor helper", Hint { desc: "Cursor 編輯器的輔助程序(擴充功能、語言伺服器等)", advice: Caution, note: "關閉可能讓擴充功能或終端機失效。建議在 Cursor 內重新載入視窗,不要直接結束" }),
    ("figma_agent", Hint { desc: "Figma 的字型輔助程式,讓網頁版 Figma 能使用本機字型", advice: Safe, note: "關閉後網頁版 Figma 無法使用本機字型,重新開啟 Figma 會自動啟動" }),
    ("devsense", Hint { desc: "DEVSENSE PHP 語言伺服器(VS Code 的 PHP 擴充功能,提供自動完成與錯誤檢查)", advice: Safe, note: "關閉後 PHP 的自動完成與診斷暫時失效,VS Code 通常會自動重新啟動它" }),
    ("postman", Hint { desc: "Postman 桌面程式的本機服務(Agent、集合執行等)", advice: Caution, note: "結束等於關閉這個 App,未送出或未儲存的內容可能遺失;建議從 App 內正常退出" }),
    ("=line", Hint { desc: "LINE 桌面版的本機服務", advice: Caution, note: "結束等於關閉 LINE 桌面版,訊息通知會中斷;建議從 App 內正常退出" }),
    ("slack", Hint { desc: "Slack 桌面版", advice: Caution, note: "結束等於關閉這個 App,未送出或未儲存的內容可能遺失;建議從 App 內正常退出" }),
    ("discord", Hint { desc: "Discord 桌面版", advice: Caution, note: "結束等於關閉這個 App,未送出或未儲存的內容可能遺失;建議從 App 內正常退出" }),
    ("zoom.us", Hint { desc: "Zoom 桌面版", advice: Caution, note: "若正在開會,結束會直接離開會議" }),
    ("notion", Hint { desc: "Notion 桌面版", advice: Caution, note: "結束等於關閉這個 App,未送出或未儲存的內容可能遺失;建議從 App 內正常退出" }),
    ("obsidian", Hint { desc: "Obsidian 筆記軟體", advice: Caution, note: "結束等於關閉這個 App,未送出或未儲存的內容可能遺失;建議從 App 內正常退出" }),
    ("telegram", Hint { desc: "Telegram 桌面版", advice: Caution, note: "結束等於關閉這個 App,未送出或未儲存的內容可能遺失;建議從 App 內正常退出" }),
    ("=claude", Hint { desc: "Claude 桌面版或 Claude Code 的本機服務", advice: Caution, note: "結束後進行中的對話或任務會中斷,建議先確認沒有正在執行的工作" }),
    ("google chrome", Hint { desc: "Google Chrome 瀏覽器(若有開 9222 之類的 port,通常是遠端偵錯,供自動化工具或擴充功能連線)", advice: Caution, note: "結束等於關閉整個 Chrome,所有分頁未儲存的內容會遺失;建議從 Chrome 內正常退出" }),
    ("spotify", Hint { desc: "Spotify 的本機連線服務(Spotify Connect、裝置探索)", advice: Safe, note: "關閉後音樂可能停止、其他裝置找不到這台電腦,重新開啟 Spotify 即可" }),
    ("dropbox", Hint { desc: "Dropbox 同步服務", advice: Caution, note: "關閉後檔案停止同步,重新開啟後會繼續;同步進行中結束可能延後完成" }),
    ("1password", Hint { desc: "1Password 與瀏覽器擴充功能溝通的本機服務", advice: Caution, note: "關閉後瀏覽器擴充功能暫時無法自動填入,重新開啟 1Password 即可" }),
    ("syncthing", Hint { desc: "Syncthing 檔案同步", advice: Safe, note: "關閉後停止同步,重新啟動會自動補同步" }),
    ("=idea", Hint { desc: "JetBrains IDE 的內建伺服器(例如 63342,供瀏覽器預覽與外掛通訊)", advice: Caution, note: "結束這個程序等於關閉 IDE,未儲存的編輯可能遺失。請從 IDE 內正常退出" }),
    ("=webstorm", Hint { desc: "JetBrains IDE 的內建伺服器(例如 63342,供瀏覽器預覽與外掛通訊)", advice: Caution, note: "結束這個程序等於關閉 IDE,未儲存的編輯可能遺失。請從 IDE 內正常退出" }),
    ("=phpstorm", Hint { desc: "JetBrains IDE 的內建伺服器(例如 63342,供瀏覽器預覽與外掛通訊)", advice: Caution, note: "結束這個程序等於關閉 IDE,未儲存的編輯可能遺失。請從 IDE 內正常退出" }),
    ("=pycharm", Hint { desc: "JetBrains IDE 的內建伺服器(例如 63342,供瀏覽器預覽與外掛通訊)", advice: Caution, note: "結束這個程序等於關閉 IDE,未儲存的編輯可能遺失。請從 IDE 內正常退出" }),
    ("=goland", Hint { desc: "JetBrains IDE 的內建伺服器(例如 63342,供瀏覽器預覽與外掛通訊)", advice: Caution, note: "結束這個程序等於關閉 IDE,未儲存的編輯可能遺失。請從 IDE 內正常退出" }),
    ("=clion", Hint { desc: "JetBrains IDE 的內建伺服器(例如 63342,供瀏覽器預覽與外掛通訊)", advice: Caution, note: "結束這個程序等於關閉 IDE,未儲存的編輯可能遺失。請從 IDE 內正常退出" }),
    ("=rider", Hint { desc: "JetBrains IDE 的內建伺服器(例如 63342,供瀏覽器預覽與外掛通訊)", advice: Caution, note: "結束這個程序等於關閉 IDE,未儲存的編輯可能遺失。請從 IDE 內正常退出" }),
    ("=rubymine", Hint { desc: "JetBrains IDE 的內建伺服器(例如 63342,供瀏覽器預覽與外掛通訊)", advice: Caution, note: "結束這個程序等於關閉 IDE,未儲存的編輯可能遺失。請從 IDE 內正常退出" }),
    ("=datagrip", Hint { desc: "JetBrains IDE 的內建伺服器(例如 63342,供瀏覽器預覽與外掛通訊)", advice: Caution, note: "結束這個程序等於關閉 IDE,未儲存的編輯可能遺失。請從 IDE 內正常退出" }),
    ("electron", Hint { desc: "Electron 桌面程式(可能是你正在開發的應用,或某個 App 的本機服務)", advice: Caution, note: "請先看執行檔路徑判斷是哪個 App;結束後該 App 的視窗可能會關閉,未儲存內容會遺失" }),
    ("tailscale", Hint { desc: "Tailscale VPN 服務", advice: Caution, note: "關閉後會中斷 Tailscale 連線,透過它的遠端存取(SSH、檔案)會斷線" }),
    ("cloudflared", Hint { desc: "Cloudflare Tunnel 客戶端,把本機服務公開到網路", advice: Caution, note: "關閉後透過該隧道公開的網址會立刻無法存取" }),
    ("ngrok", Hint { desc: "ngrok 隧道,把本機服務臨時公開到網路", advice: Safe, note: "關閉後公開網址失效,重新執行會得到新網址(免費版網址會變)" }),
    ("openvpn", Hint { desc: "OpenVPN 連線", advice: Caution, note: "關閉後 VPN 中斷,依賴 VPN 的連線會失敗" }),
    ("=ssh", Hint { desc: "SSH 連線(常見用途是 -L / -R 的 port 轉發)", advice: Safe, note: "關閉後該連線與轉發中斷,需要時重新連線即可。若這條連線正在操作遠端主機,遠端進行中的工作可能受影響" }),
    ("=kubectl", Hint { desc: "kubectl port-forward,把 Kubernetes 的服務轉發到本機", advice: Safe, note: "關閉後轉發中斷,需要時重新執行 port-forward" }),
    ("=nc", Hint { desc: "netcat,手動開的網路監聽 / 連線", advice: Safe, note: "通常是臨時測試用,關閉即可" }),
    ("=ncat", Hint { desc: "ncat,手動開的網路監聽 / 連線", advice: Safe, note: "通常是臨時測試用,關閉即可" }),
    ("=socat", Hint { desc: "socat 網路轉發工具", advice: Safe, note: "關閉後它轉發的連線會中斷,通常是手動開的,關閉即可" }),
    ("orbstack", Hint { desc: "OrbStack(Docker / Linux 虛擬機)的後端", advice: Caution, note: "結束後所有容器與虛擬機的 port 會中斷,建議從 OrbStack 選單退出" }),
    ("colima", Hint { desc: "Colima(Docker 虛擬機)", advice: Caution, note: "結束後容器與 port 中斷,建議用 colima stop 正常停止" }),
    ("vpnkit", Hint { desc: "Docker Desktop 的網路元件", advice: Caution, note: "關閉會影響容器的網路,建議從 Docker Desktop 操作" }),
    ("docker", Hint { desc: "Docker 相關程序", advice: Caution, note: "可能影響執行中的容器,建議從 Docker Desktop 操作" }),
    ("qemu-system", Hint { desc: "QEMU 虛擬機(UTM、Android 模擬器等會用到)", advice: Caution, note: "關閉等於強制關機該虛擬機,裡面未儲存的資料會遺失。請從 UTM / 模擬器內正常關機" }),
    ("adb", Hint { desc: "Android Debug Bridge 伺服器,用來連接 Android 手機或模擬器做開發除錯", advice: Safe, note: "不影響手機本身。下次執行 adb 指令時會自動重新啟動;若正在除錯,連線會中斷" }),
    ("postgres", Hint { desc: "PostgreSQL 資料庫伺服器", advice: Caution, note: "強制結束可能造成資料庫需要復原。請用 brew services stop 或資料庫的正常關閉方式,避免使用強制結束,並確認沒有程式正在連線" }),
    ("mysqld", Hint { desc: "MySQL 資料庫伺服器", advice: Caution, note: "強制結束可能造成資料損壞或復原等待。請用正常方式關閉,避免強制結束" }),
    ("mariadbd", Hint { desc: "MariaDB 資料庫伺服器", advice: Caution, note: "強制結束可能造成資料損壞或復原等待。請用正常方式關閉,避免強制結束" }),
    ("mongod", Hint { desc: "MongoDB 資料庫伺服器", advice: Caution, note: "強制結束可能造成資料損壞或復原等待。請用正常方式關閉,避免強制結束" }),
    ("clickhouse", Hint { desc: "ClickHouse 資料庫伺服器", advice: Caution, note: "強制結束可能造成資料需要復原,請用正常方式關閉" }),
    ("influxd", Hint { desc: "InfluxDB 時序資料庫", advice: Caution, note: "強制結束可能造成最近寫入的資料遺失,請用正常方式關閉" }),
    ("redis", Hint { desc: "Redis 記憶體資料庫 / 快取伺服器", advice: Caution, note: "若沒有開啟持久化,快取與佇列內容會遺失;依賴它的程式會連線失敗" }),
    ("valkey", Hint { desc: "Valkey(Redis 相容)記憶體資料庫 / 快取伺服器", advice: Caution, note: "若沒有開啟持久化,快取與佇列內容會遺失;依賴它的程式會連線失敗" }),
    ("memcached", Hint { desc: "Memcached 快取伺服器", advice: Safe, note: "只會清空快取內容,依賴它的程式會變慢或重建快取,通常無害" }),
    ("beam.smp", Hint { desc: "Erlang 虛擬機,常見於 RabbitMQ、CouchDB、Elixir 應用", advice: Caution, note: "若是 RabbitMQ,佇列中未處理的訊息可能遺失;請從服務本身的方式正常停止" }),
    ("minio", Hint { desc: "MinIO 物件儲存(相容 S3)", advice: Caution, note: "關閉後依賴它存取檔案的程式會失敗,強制結束可能造成寫入中的檔案不完整" }),
    ("meilisearch", Hint { desc: "Meilisearch 搜尋引擎", advice: Caution, note: "關閉後搜尋功能失效,重新啟動後索引會自動載入" }),
    ("typesense", Hint { desc: "Typesense 搜尋引擎", advice: Caution, note: "關閉後搜尋功能失效,重新啟動後索引會自動載入" }),
    ("etcd", Hint { desc: "etcd 分散式鍵值儲存", advice: Caution, note: "若是 Kubernetes 或其他叢集的設定儲存,關閉會讓叢集失效,請確認用途" }),
    ("=consul", Hint { desc: "Consul 服務發現 / 設定儲存", advice: Caution, note: "關閉後依賴它做服務發現的程式會失敗" }),
    ("=vault", Hint { desc: "HashiCorp Vault 密鑰管理", advice: Caution, note: "關閉後依賴它取得密鑰的程式會失敗;若是開發模式(-dev),資料不會保留" }),
    ("grafana", Hint { desc: "Grafana 儀表板", advice: Safe, note: "只是儀表板介面,關閉不影響被監控的服務,重新啟動即可" }),
    ("prometheus", Hint { desc: "Prometheus 監控", advice: Caution, note: "關閉期間不會收集指標,會出現資料空窗" }),
    ("traefik", Hint { desc: "Traefik 反向代理 / 邊緣路由", advice: Caution, note: "關閉後由它轉發的服務會無法連線" }),
    ("haproxy", Hint { desc: "HAProxy 負載平衡 / 反向代理", advice: Caution, note: "關閉後由它轉發的服務會無法連線" }),
    ("envoy", Hint { desc: "Envoy 代理", advice: Caution, note: "關閉後由它轉發的服務會無法連線" }),
    ("nginx", Hint { desc: "Nginx 網頁伺服器 / 反向代理", advice: Caution, note: "關閉後由它提供的網站或代理會無法連線。若只是本機開發用則可安全關閉" }),
    ("caddy", Hint { desc: "Caddy 網頁伺服器 / 反向代理", advice: Caution, note: "關閉後由它提供的網站或代理會無法連線。若只是本機開發用則可安全關閉" }),
    ("apache2", Hint { desc: "Apache 網頁伺服器", advice: Caution, note: "關閉後由它提供的網站會無法連線。若只是本機開發用則可安全關閉" }),
    ("httpd", Hint { desc: "Apache 網頁伺服器", advice: Caution, note: "關閉後由它提供的網站會無法連線。若只是本機開發用則可安全關閉" }),
    ("mailpit", Hint { desc: "Mailpit,開發用的郵件測試伺服器(SMTP 與網頁收件匣)", advice: Safe, note: "關閉後收不到測試信,已收的信件會消失,開發用途無害" }),
    ("mailhog", Hint { desc: "MailHog,開發用的郵件測試伺服器", advice: Safe, note: "關閉後收不到測試信,已收的信件會消失,開發用途無害" }),
    ("gitea", Hint { desc: "Gitea 自架 Git 服務", advice: Caution, note: "關閉後無法 push / pull 這個服務上的儲存庫" }),
    ("forgejo", Hint { desc: "Forgejo 自架 Git 服務", advice: Caution, note: "關閉後無法 push / pull 這個服務上的儲存庫" }),
    ("pocketbase", Hint { desc: "PocketBase 後端(資料庫加 API)", advice: Caution, note: "關閉後依賴它的應用會連線失敗,資料保存在 pb_data 資料夾,不會遺失" }),
    ("ollama", Hint { desc: "Ollama,在本機執行大型語言模型的服務", advice: Safe, note: "會卸載已載入的模型,進行中的對話會中斷;下次呼叫時會自動重新載入。若是以 App 常駐,可能被自動重啟" }),
    ("php", Hint { desc: "PHP 程式(開發伺服器或 php-fpm)。看「工作目錄」可知是哪個專案", advice: Caution, note: "若是 php artisan serve 之類的開發伺服器可直接關閉;若是 php-fpm,關閉後網站會停止回應" }),
    ("python", Hint { desc: "Python 程式(腳本或開發伺服器)。看「指令列」與「工作目錄」可知是哪個專案", advice: Safe, note: "開發伺服器直接關閉即可。若它正在跑資料匯入、訓練、爬蟲等長時間任務,進度可能遺失" }),
    ("node", Hint { desc: "Node.js 程式,通常是前端開發伺服器(Vite、Next.js 等)。看「工作目錄」可知是哪個專案", advice: Safe, note: "開發伺服器直接關閉即可,重新執行 dev 指令就會回來。若是在跑建置或其他長時間任務,進度可能遺失" }),
    ("deno", Hint { desc: "Deno 程式,通常是開發伺服器。看「工作目錄」可知是哪個專案", advice: Safe, note: "開發伺服器直接關閉即可,重新執行就會回來" }),
    ("=bun", Hint { desc: "Bun 程式,通常是開發伺服器。看「工作目錄」可知是哪個專案", advice: Safe, note: "開發伺服器直接關閉即可,重新執行就會回來" }),
    ("dotnet", Hint { desc: ".NET 程式(ASP.NET 開發伺服器等)。看「指令列」與「工作目錄」可知是哪個專案", advice: Safe, note: "開發伺服器直接關閉即可,重新執行 dotnet run 就會回來" }),
    ("java", Hint { desc: "Java 程式(Spring Boot、Gradle 等)。看「指令列」可知是哪個應用", advice: Caution, note: "可能是應用伺服器或 Gradle daemon。關閉前確認沒有進行中的建置或請求" }),
    ("ruby", Hint { desc: "Ruby 程式(Rails 等)。看「工作目錄」可知是哪個專案", advice: Safe, note: "開發伺服器直接關閉即可,重新啟動就會回來" }),
    ("=hugo", Hint { desc: "Hugo 靜態網站開發伺服器", advice: Safe, note: "直接關閉即可,預覽頁面會斷線" }),
    ("=trunk", Hint { desc: "Trunk(Rust WebAssembly 前端)開發伺服器", advice: Safe, note: "直接關閉即可,重新執行就會回來" }),
];

/// 依指令列辨識具體的開發工具(Node / Python 等執行環境的指令名稱都一樣,只能看參數)。
/// key 無前綴 = 指令列中的完整單字(以空白、`/`、`:` 切開);`~` 開頭 = 指令列包含該字串。
const ARG_HINTS: &[(&str, Hint)] = &[
    ("vite", Hint { desc: "Vite 開發伺服器(前端專案的熱更新預覽)", advice: Safe, note: "開發伺服器直接關閉即可,重新執行對應指令就會回來;瀏覽器預覽會斷線" }),
    ("next-server", Hint { desc: "Next.js 伺服器", advice: Safe, note: "開發伺服器直接關閉即可,重新執行對應指令就會回來;瀏覽器預覽會斷線" }),
    ("next", Hint { desc: "Next.js 開發伺服器", advice: Safe, note: "開發伺服器直接關閉即可,重新執行對應指令就會回來;瀏覽器預覽會斷線" }),
    ("nuxt", Hint { desc: "Nuxt 開發伺服器", advice: Safe, note: "開發伺服器直接關閉即可,重新執行對應指令就會回來;瀏覽器預覽會斷線" }),
    ("nuxi", Hint { desc: "Nuxt 開發伺服器", advice: Safe, note: "開發伺服器直接關閉即可,重新執行對應指令就會回來;瀏覽器預覽會斷線" }),
    ("astro", Hint { desc: "Astro 開發伺服器", advice: Safe, note: "開發伺服器直接關閉即可,重新執行對應指令就會回來;瀏覽器預覽會斷線" }),
    ("svelte-kit", Hint { desc: "SvelteKit 開發伺服器", advice: Safe, note: "開發伺服器直接關閉即可,重新執行對應指令就會回來;瀏覽器預覽會斷線" }),
    ("webpack-dev-server", Hint { desc: "Webpack 開發伺服器", advice: Safe, note: "開發伺服器直接關閉即可,重新執行對應指令就會回來;瀏覽器預覽會斷線" }),
    ("react-scripts", Hint { desc: "Create React App 開發伺服器", advice: Safe, note: "開發伺服器直接關閉即可,重新執行對應指令就會回來;瀏覽器預覽會斷線" }),
    ("storybook", Hint { desc: "Storybook 元件預覽伺服器", advice: Safe, note: "開發伺服器直接關閉即可,重新執行對應指令就會回來;瀏覽器預覽會斷線" }),
    ("remix", Hint { desc: "Remix 開發伺服器", advice: Safe, note: "開發伺服器直接關閉即可,重新執行對應指令就會回來;瀏覽器預覽會斷線" }),
    ("gatsby", Hint { desc: "Gatsby 開發伺服器", advice: Safe, note: "開發伺服器直接關閉即可,重新執行對應指令就會回來;瀏覽器預覽會斷線" }),
    ("expo", Hint { desc: "Expo(React Native)開發伺服器", advice: Safe, note: "關閉後手機 / 模擬器上的開發中 App 會斷線,重新執行 expo start 即可" }),
    ("tauri", Hint { desc: "Tauri 開發模式的前端伺服器", advice: Safe, note: "開發伺服器直接關閉即可,重新執行對應指令就會回來;瀏覽器預覽會斷線" }),
    ("nodemon", Hint { desc: "nodemon,檔案變動時自動重啟的 Node.js 開發伺服器", advice: Safe, note: "開發伺服器直接關閉即可,重新執行對應指令就會回來;瀏覽器預覽會斷線" }),
    ("live-server", Hint { desc: "live-server 靜態網頁預覽", advice: Safe, note: "開發伺服器直接關閉即可,重新執行對應指令就會回來;瀏覽器預覽會斷線" }),
    ("http-server", Hint { desc: "http-server 靜態網頁伺服器", advice: Safe, note: "開發伺服器直接關閉即可,重新執行對應指令就會回來;瀏覽器預覽會斷線" }),
    ("json-server", Hint { desc: "json-server 假資料 API", advice: Safe, note: "開發伺服器直接關閉即可,重新執行對應指令就會回來;瀏覽器預覽會斷線" }),
    ("wrangler", Hint { desc: "Cloudflare Wrangler 本機開發伺服器(Workers / Pages)", advice: Safe, note: "開發伺服器直接關閉即可,重新執行對應指令就會回來;瀏覽器預覽會斷線" }),
    ("firebase", Hint { desc: "Firebase 模擬器 / 本機開發伺服器", advice: Safe, note: "關閉後模擬器內的資料會消失(除非有匯出),重新執行即可" }),
    ("uvicorn", Hint { desc: "Uvicorn,Python ASGI 伺服器(FastAPI 等)", advice: Safe, note: "開發伺服器直接關閉即可,重新執行對應指令就會回來;瀏覽器預覽會斷線" }),
    ("gunicorn", Hint { desc: "Gunicorn,Python WSGI 伺服器", advice: Caution, note: "若是正式環境的服務,關閉後網站會停止回應;本機開發用則可直接關閉" }),
    ("hypercorn", Hint { desc: "Hypercorn,Python ASGI 伺服器", advice: Safe, note: "開發伺服器直接關閉即可,重新執行對應指令就會回來;瀏覽器預覽會斷線" }),
    ("flask", Hint { desc: "Flask 開發伺服器", advice: Safe, note: "開發伺服器直接關閉即可,重新執行對應指令就會回來;瀏覽器預覽會斷線" }),
    ("fastapi", Hint { desc: "FastAPI 開發伺服器", advice: Safe, note: "開發伺服器直接關閉即可,重新執行對應指令就會回來;瀏覽器預覽會斷線" }),
    ("manage.py", Hint { desc: "Django 開發伺服器(manage.py runserver)", advice: Safe, note: "開發伺服器直接關閉即可,重新執行對應指令就會回來;瀏覽器預覽會斷線" }),
    ("streamlit", Hint { desc: "Streamlit 應用", advice: Safe, note: "開發伺服器直接關閉即可,重新執行對應指令就會回來;瀏覽器預覽會斷線" }),
    ("gradio", Hint { desc: "Gradio 應用", advice: Safe, note: "開發伺服器直接關閉即可,重新執行對應指令就會回來;瀏覽器預覽會斷線" }),
    ("jupyter", Hint { desc: "Jupyter Notebook / Lab 伺服器", advice: Caution, note: "關閉後 Notebook 的執行狀態與變數會消失,未儲存的內容會遺失,請先儲存" }),
    ("jupyter-lab", Hint { desc: "JupyterLab 伺服器", advice: Caution, note: "關閉後 Notebook 的執行狀態與變數會消失,未儲存的內容會遺失,請先儲存" }),
    ("jupyter-notebook", Hint { desc: "Jupyter Notebook 伺服器", advice: Caution, note: "關閉後 Notebook 的執行狀態與變數會消失,未儲存的內容會遺失,請先儲存" }),
    ("tensorboard", Hint { desc: "TensorBoard 訓練視覺化", advice: Safe, note: "只是檢視介面,關閉不影響訓練本身" }),
    ("mlflow", Hint { desc: "MLflow 實驗追蹤伺服器", advice: Caution, note: "關閉後無法記錄或查看實驗結果,資料保留在後端儲存,不會遺失" }),
    ("mkdocs", Hint { desc: "MkDocs 文件預覽伺服器", advice: Safe, note: "開發伺服器直接關閉即可,重新執行對應指令就會回來;瀏覽器預覽會斷線" }),
    ("http.server", Hint { desc: "Python 內建的簡易網頁伺服器(python -m http.server)", advice: Safe, note: "通常是臨時分享檔案用,關閉即可" }),
    ("artisan", Hint { desc: "Laravel 開發伺服器(php artisan serve)", advice: Safe, note: "開發伺服器直接關閉即可,重新執行對應指令就會回來;瀏覽器預覽會斷線" }),
    ("reverb", Hint { desc: "Laravel Reverb WebSocket 伺服器", advice: Safe, note: "關閉後即時通訊功能暫停,重新啟動即可" }),
    ("rails", Hint { desc: "Rails 開發伺服器", advice: Safe, note: "開發伺服器直接關閉即可,重新執行對應指令就會回來;瀏覽器預覽會斷線" }),
    ("puma", Hint { desc: "Puma,Ruby 網頁伺服器", advice: Safe, note: "開發伺服器直接關閉即可,重新執行對應指令就會回來;瀏覽器預覽會斷線" }),
    ("jekyll", Hint { desc: "Jekyll 靜態網站開發伺服器", advice: Safe, note: "開發伺服器直接關閉即可,重新執行對應指令就會回來;瀏覽器預覽會斷線" }),
    ("~gradledaemon", Hint { desc: "Gradle daemon,加速 Gradle 建置的背景程序", advice: Safe, note: "關閉無妨,下次建置會自動重新啟動(第一次會較慢)" }),
    ("~spring-boot", Hint { desc: "Spring Boot 應用", advice: Caution, note: "若是開發中的服務可直接關閉;關閉前確認沒有進行中的請求或交易" }),
    ("~elasticsearch", Hint { desc: "Elasticsearch 搜尋引擎", advice: Caution, note: "關閉後搜尋功能失效,強制結束可能造成索引需要復原,請用正常方式停止" }),
    ("~kafka", Hint { desc: "Apache Kafka", advice: Caution, note: "關閉後依賴訊息佇列的程式會失敗,強制結束可能造成日誌需要復原" }),
    ("~jenkins", Hint { desc: "Jenkins CI 伺服器", advice: Caution, note: "關閉後建置工作停止,進行中的建置會失敗" }),
    ("~catalina", Hint { desc: "Apache Tomcat 應用伺服器", advice: Caution, note: "關閉後部署在上面的應用都會無法連線" }),
];

fn key_matches(key: &str, command: &str, exe: &str) -> bool {
    match key.strip_prefix('=') {
        Some(exact) => command == exact || exe == exact,
        None => command.contains(key) || exe.contains(key),
    }
}

fn find_hint(command: &str, exe: &str) -> Option<&'static (&'static str, Hint)> {
    let c = command.replace("\\x20", " ").to_lowercase();
    // 只比對檔名,避免路徑中的資料夾名稱(如 python-projects)造成誤判
    let e = exe.rsplit('/').next().unwrap_or(exe).to_lowercase();
    HINTS.iter().find(|(k, _)| key_matches(k, &c, &e))
}

pub fn hint_for(command: &str, exe: &str) -> Option<&'static Hint> {
    find_hint(command, exe).map(|(_, h)| h)
}

fn hint_for_args(args: &str) -> Option<&'static Hint> {
    let lower = args.to_lowercase();
    let words: Vec<&str> = lower
        .split(|ch: char| ch.is_whitespace() || ch == '/' || ch == ':')
        .filter(|w| !w.is_empty())
        .collect();
    ARG_HINTS
        .iter()
        .find(|(k, _)| match k.strip_prefix('~') {
            Some(sub) => lower.contains(sub),
            None => words.contains(k),
        })
        .map(|(_, h)| h)
}

/// 語言執行環境:指令名稱都一樣(node、python…),要看指令列才知道實際跑的是什麼
const RUNTIME_KEYS: &[&str] = &["php", "python", "node", "deno", "=bun", "dotnet", "java", "ruby"];

/// 先依指令列辨識具體工具,認不出來再用指令名稱 / 執行檔比對
pub fn hint_for_process(command: &str, exe: &str, args: &str) -> Option<&'static Hint> {
    match find_hint(command, exe) {
        // 只有執行環境才讓指令列細分;資料庫、App、系統服務維持各自的專屬說明
        Some((k, base)) if !RUNTIME_KEYS.contains(k) => Some(base),
        Some((_, base)) => hint_for_args(args).or(Some(base)),
        None => hint_for_args(args),
    }
}

/// 綜合判斷關閉建議。安全規則優先於個別程式的說明:
/// 別的使用者(含 root)的進程、macOS 系統路徑下的程式,一律不建議關閉。
pub fn assess(hint: Option<(Advice, &str)>, user: &str, current_user: &str, exe: &str) -> (Advice, String) {
    if !current_user.is_empty() && user != current_user {
        return (
            Avoid,
            format!("這個進程屬於「{user}」而不是你,通常是系統服務,結束它需要管理員權限,而且可能影響系統功能"),
        );
    }
    const SYSTEM_PATHS: &[&str] = &["/System/", "/usr/libexec/", "/usr/sbin/", "/sbin/"];
    if SYSTEM_PATHS.iter().any(|p| exe.starts_with(p)) {
        let extra = hint.map(|h| h.1).filter(|n| !n.is_empty()).unwrap_or("結束後 macOS 通常會自動重啟它,功能可能暫時失效");
        return (Avoid, format!("這是 macOS 系統程式。{extra}"));
    }
    match hint {
        Some((advice, note)) => (advice, note.to_string()),
        None => (
            Caution,
            "不確定這是什麼程式。請先看執行檔路徑與工作目錄確認來源;如果是你自己啟動的,結束後重新執行即可回復,但未儲存的資料會遺失".to_string(),
        ),
    }
}

fn current_user() -> String {
    Command::new("id")
        .arg("-un")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn ps_field(pid: u32, field: &str) -> String {
    Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", &format!("{field}=")])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn cwd_of(pid: u32) -> String {
    Command::new("lsof")
        .args(["-a", "-p", &pid.to_string(), "-d", "cwd", "-Fn"])
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .find_map(|l| l.strip_prefix('n').map(str::to_string))
                .unwrap_or_default()
        })
        .unwrap_or_default()
}

fn default_note(advice: Advice) -> &'static str {
    match advice {
        Advice::Safe => "你將這個程式標記為可以關閉",
        Advice::Caution => "你將這個程式標記為關閉前要留意",
        Advice::Avoid => "你將這個程式標記為不建議關閉",
    }
}

fn is_generic_cwd(cwd: &str) -> bool {
    let home = std::env::var("HOME").unwrap_or_default();
    cwd.is_empty()
        || cwd == "/"
        || (!home.is_empty() && cwd.trim_end_matches('/') == home.trim_end_matches('/'))
        || ["/System", "/usr", "/Library", "/private", "/Applications", "/opt"]
            .iter()
            .any(|p| cwd == *p || cwd.starts_with(&format!("{p}/")))
}

/// 查詢正在監聽 port 的進程詳情;只接受目前確實在監聽的 pid
pub fn process_info(pid: u32) -> Result<ProcessInfo, String> {
    let all = listeners()?;
    let l = all
        .iter()
        .find(|l| l.pid == pid)
        .ok_or_else(|| format!("pid {pid} 已不在監聽任何 port"))?;
    let args = ps_field(pid, "args");
    if args.is_empty() {
        return Err(format!("找不到 pid {pid},進程可能已結束"));
    }
    let exe = ps_field(pid, "comm");
    let user = ps_field(pid, "user");
    let cwd = cwd_of(pid);

    // 使用者自訂優先於內建;設定檔壞掉時退回內建,不讓查詢整個失敗
    let rules = custom::list_rules().unwrap_or_default();
    let custom = custom::find_rule(&rules, &l.command, &args, &cwd);
    let builtin = hint_for_process(&l.command, &exe, &args);
    let (hint, resolved): (Option<String>, Option<(Advice, String)>) = match (custom, builtin) {
        (Some(r), _) => {
            let note = if r.note.is_empty() { default_note(r.advice).to_string() } else { r.note.clone() };
            (Some(r.desc.clone()), Some((r.advice, note)))
        }
        (None, Some(h)) => (Some(h.desc.to_string()), Some((h.advice, h.note.to_string()))),
        (None, None) => (None, None),
    };
    let (advice, advice_note) = assess(
        resolved.as_ref().map(|(a, n)| (*a, n.as_str())),
        &user,
        &current_user(),
        &exe,
    );
    Ok(ProcessInfo {
        pid,
        user,
        started: ps_field(pid, "lstart"),
        hint,
        advice,
        advice_note,
        custom_id: custom.map(|r| r.id.clone()),
        cwd_generic: is_generic_cwd(&cwd),
        exe,
        args,
        cwd,
    })
}

pub fn parse_port(s: &str) -> Result<u16, String> {
    s.parse().map_err(|_| format!("無效的 port: {s}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_dedupes_listeners() {
        let text = "p100\ncnode\nn*:3000\nn[::1]:3000\np200\ncpostgres\nn127.0.0.1:5432\n";
        let got = parse_lsof(text);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].port, 3000);
        assert_eq!(got[0].command, "node");
        assert_eq!(got[1].port, 5432);
        assert_eq!(got[1].pid, 200);
    }

    #[test]
    fn ignores_non_port_and_empty_input() {
        assert!(parse_lsof("").is_empty());
        assert!(parse_lsof("p1\ncfoo\nnnot-a-port\n").is_empty());
    }

    #[test]
    fn hints_match_command_or_path() {
        assert!(hint_for("com.docker.backend", "").unwrap().desc.contains("Docker"));
        assert!(hint_for("Code\\x20Helper", "").unwrap().desc.contains("VS Code"));
        assert!(hint_for("x", "/usr/local/bin/node").unwrap().desc.contains("Node.js"));
        assert!(hint_for("mystery", "/opt/mystery").is_none());
        assert!(hint_for("mystery", "/Users/me/python-projects/mystery").is_none());
    }

    #[test]
    fn exact_keys_do_not_match_substrings() {
        assert!(hint_for("ssh", "").unwrap().desc.contains("SSH 連線"));
        assert!(hint_for("sshd", "").unwrap().desc.contains("遠端登入"));
        assert!(hint_for("phpstorm", "").unwrap().desc.contains("JetBrains"));
        assert!(hint_for("bundled", "").is_none());
    }

    #[test]
    fn app_names_win_over_language_substrings() {
        // 名稱含 php,但其實是 VS Code 擴充功能的語言伺服器,不是 PHP 開發伺服器
        assert!(hint_for("devsense.php.ls", "").unwrap().desc.contains("語言伺服器"));
        assert!(hint_for("Postman", "").unwrap().desc.contains("Postman"));
        assert!(hint_for("LINE", "").unwrap().desc.contains("LINE"));
        assert!(hint_for("Google\\x20Chrome", "").unwrap().desc.contains("Chrome"));
        // =line 是完全比對,不影響名稱含 line 的其他程式
        assert!(hint_for("pipeline-runner", "").is_none());
    }

    #[test]
    fn args_identify_specific_dev_tools() {
        let node = |a| hint_for_process("node", "/usr/local/bin/node", a).unwrap().desc;
        assert!(node("node /app/node_modules/.bin/vite --port 5173").contains("Vite"));
        assert!(node("next-server (v15.1.0)").contains("Next.js"));
        assert!(node("node server.js").contains("Node.js")); // 認不出具體工具 → 通用說明
        let py = |a| hint_for_process("Python", "/usr/bin/python3", a).unwrap().desc;
        assert!(py("python3 -m uvicorn main:app").contains("Uvicorn"));
        assert!(py("python3 -m http.server 8000").contains("http.server"));
        assert!(hint_for_process("php", "/usr/bin/php", "php artisan serve").unwrap().desc.contains("Laravel"));
    }

    #[test]
    fn specific_programs_are_not_overridden_by_args() {
        // 資料庫的指令列即使出現 vite 之類字眼,也維持資料庫的說明
        let h = hint_for_process("postgres", "/opt/pg/postgres", "postgres -D /data/vite").unwrap();
        assert!(h.desc.contains("PostgreSQL"));
    }

    #[test]
    fn assess_prefers_safety_rules() {
        let node = hint_for("node", "");
        // 一般使用者的 node → 沿用說明表
        assert_eq!(assess(node.map(|h| (h.advice, h.note)), "me", "me", "/usr/local/bin/node").0, Safe);
        // root 的進程 → 不建議
        assert_eq!(assess(node.map(|h| (h.advice, h.note)), "root", "me", "/usr/local/bin/node").0, Avoid);
        // 系統路徑 → 不建議
        assert_eq!(assess(None, "me", "me", "/System/Library/CoreServices/x").0, Avoid);
        // 未知程式 → 留意
        assert_eq!(assess(None, "me", "me", "/opt/foo").0, Caution);
        // 取不到目前使用者時不誤判為「別人的進程」
        assert_eq!(assess(node.map(|h| (h.advice, h.note)), "me", "", "/usr/local/bin/node").0, Safe);
    }

    #[test]
    fn rejects_invalid_port() {
        assert!(parse_port("abc").is_err());
        assert!(parse_port("70000").is_err());
        assert_eq!(parse_port("8080"), Ok(8080));
    }
}
