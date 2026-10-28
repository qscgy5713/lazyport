# lazyport

查看本機 TCP port 被誰佔用,並可一鍵結束該進程的小工具(macOS),提供 CLI 與桌面應用兩種介面。

## 技術棧
- 核心:Rust 2021 函式庫(依賴 `serde` / `serde_json`,用於自訂說明設定檔),透過系統內建的 `lsof`、`ps`、`kill`
- CLI:`src/main.rs`
- 桌面版:Tauri 2 + Svelte 5 + Vite(`desktop/`),共用核心函式庫

## 安裝與環境設定
需要 Rust(`cargo`)。

```bash
cd ~/www/lazyport
cargo build --release
# 選用:放進 PATH
cp target/release/lazyport /usr/local/bin/
```

## 使用方式
```bash
lazyport                 # 列出所有監聽中的 TCP port
lazyport 3000            # 只看 3000
lazyport info 3000       # 這個 port 是什麼程式、從哪裡啟動、建議關閉嗎
lazyport kill 3000       # 結束佔用 3000 的進程(SIGTERM)
lazyport kill 3000 -f    # 強制結束
lazyport kill 3000 --yes # 對「不建議關閉」的程式也照樣結束
lazyport hint list       # 列出你的自訂說明
lazyport hint add args aitrader "我的 AI 交易機器人" --advice avoid --note "關閉後停止下單"
lazyport hint rm <id>    # 刪除自訂說明(SIGKILL)
lazyport -h              # 說明
```

### 桌面版
需要 Node.js 與 Rust。

```bash
cd desktop
npm install
npm run tauri dev      # 開發模式
npm run tauri build -- --bundles app   # 只打包 .app
# 產物:desktop/src-tauri/target/release/bundle/macos/lazyport.app(約 10 MB)
# 安裝:拖進「應用程式」;未簽署,第一次請右鍵 → 打開
```
功能:列出監聽 port、搜尋、每 3 秒自動更新、兩段式確認後結束進程(可勾選強制結束);對「不建議關閉」的程式(系統服務、他人進程)會展開紅色警告,說明影響後需按「我了解風險,仍要結束」才會執行。

程式說明:點擊程式名稱展開詳情,包含白話說明(內建常見程式對照表)、「建議關閉嗎」與關閉影響(可以關閉 / 關閉前請留意 / 不建議關閉)、執行檔、完整指令列、工作目錄、使用者與啟動時間。

自訂說明:展開詳情後按「＋ 自訂說明」,可用「工作目錄 / 指令列包含 / 程式名稱」比對,並設定說明、建議等級與關閉影響。
- 存放於 `~/.config/lazyport/hints.json`(可用環境變數 `LAZYPORT_CONFIG_DIR` 改目錄),桌面版與 CLI 共用
- 優先於內建說明;但系統程式與他人進程的「不建議關閉」安全規則不會被蓋過
- 標成「不建議關閉」的自訂程式,按結束時同樣會出現加強警告
- 視窗底部「自訂說明」可查看與刪除全部規則

選單列常駐:
- 啟動後只出現在選單列(不佔 Dock),左鍵點圖示顯示/隱藏視窗,右鍵開選單
- 全域快捷鍵預設 `⌘⇧P` 顯示/隱藏視窗,可在視窗下方按「變更」後直接按新組合鍵(需含 ⌘/⌃/⌥ 之一;設定存在 `~/Library/Application Support/dev.lazyport.desktop/settings.json`)
- 選單列使用單色 template 圖示,自動配合淺色/深色選單列
- 關閉視窗只是隱藏;要完全結束請從選單列選「結束 lazyport」
- 啟動(或再次點擊 App 圖示)時會自動把視窗帶到最前面;因為沒有 Dock 圖示,否則視窗可能被其他視窗擋住

## 測試
```bash
cargo test
```

## 目錄結構
```
lazyport/
├── Cargo.toml
├── src/lib.rs       # 核心:lsof 解析、程式說明與關閉建議、結束進程(含單元測試)
├── src/custom.rs    # 自訂說明:驗證、比對、讀寫設定檔(含單元測試)
├── src/main.rs      # CLI
├── desktop/         # Tauri + Svelte 桌面版
│   ├── src/         # Svelte 前端
│   └── src-tauri/   # Tauri Rust 後端(呼叫核心函式庫)
├── README.md
└── doc/
    ├── worklog.md
    ├── todo.md
    └── plan.md
```

## 文件索引
- [工作日誌](doc/worklog.md)
- [待辦清單](doc/todo.md)
- [計畫與設計](doc/plan.md)
