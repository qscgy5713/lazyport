# 計畫與設計

## 目標
一個快速查 port 佔用並結束進程的 CLI,取代每次手打 `lsof -i :PORT` 加 `kill`。

## 範圍
- 僅 macOS(依賴 `lsof`、`kill`)、僅 TCP LISTEN
- 零外部依賴,單一執行檔

## 設計
- 呼叫 `lsof -nP -iTCP -sTCP:LISTEN -Fpcn`,解析 p/c/n 欄位
- IPv4/IPv6 重複的 (pid, port) 只留一筆,依 port 排序
- `kill` 預設 SIGTERM,`-f` 改 SIGKILL

## 取捨
- 用 `kill` 指令而非 libc,避免外部依賴
- 只看當前使用者可見的進程;他人進程需 sudo

## 桌面版(Tauri + Svelte)
- 核心拆成 `src/lib.rs`,CLI 與桌面版共用,避免重複邏輯
- Tauri 指令只有 `list_ports`、`kill_port(port, force)`;結束進程時由核心層重新查出正在監聽該 port 的 pid,前端無法指定任意 pid
- lsof / kill 以 `spawn_blocking` 執行,避免卡住 UI
- 前端選 Svelte:檔案少、體積小;結束按鈕採兩段式確認
- CSP 限制為 `default-src 'self'`

## 選單列常駐
- Tauri `tray-icon` + `tauri-plugin-global-shortcut`,全在 Rust 端註冊,前端不需額外權限
- macOS 設為 Accessory(無 Dock 圖示);關閉視窗改為隱藏
- 切換邏輯:視窗在前景則隱藏,否則顯示並取得焦點
- 快捷鍵註冊失敗只記錄訊息,不中斷啟動

## 程式詳情
- 核心 `process_info(pid)`:`ps` 取使用者/啟動時間/執行檔/指令列,`lsof -d cwd` 取工作目錄
- 只回答「目前正在監聽 port」的 pid,不開放查任意進程
- `HINTS` 對照表依指令名稱或執行檔「檔名」比對(不比對路徑,避免資料夾名稱誤判),未命中則提示看路徑判斷

## 自訂快捷鍵
- 設定存在 app config 目錄的 settings.json;啟動時讀取,無效則回到預設
- 變更流程:先註冊新的 → 存檔成功 → 才取消舊的,任一步失敗維持原快捷鍵
- 前端以 KeyboardEvent.code 轉換,需含 ⌘/⌃/⌥ 之一,支援字母、數字、F 鍵、空白鍵

## 關閉建議
- 三級:可以關閉(Safe)/ 關閉前請留意(Caution)/ 不建議關閉(Avoid)
- 判斷順序:① 進程屬於別的使用者(含 root)→ Avoid ② 執行檔在 /System、/usr/libexec、/usr/sbin、/sbin → Avoid ③ 套用 `HINTS` 該程式的等級與影響說明 ④ 未收錄 → Caution
- 安全規則優先於個別程式說明;取不到目前使用者時不套用規則①,避免誤判
- 建議只是參考,不會阻擋使用者操作;結束仍維持兩段式確認

## 加強警告
- 桌面版:按「結束」先重新查詢該進程的建議(不使用快取,避免 pid 被重新分配);查詢期間按鈕停用,避免來不及警告就被連點兩下
- Avoid → 展開紅色警告列(顯示影響;若勾選強制結束再加註風險),需按「我了解風險,仍要結束」;其餘等級維持原本的兩段式確認
- 查詢失敗時退回一般確認流程,不阻擋使用者
- CLI:Avoid 的程式預設拒絕並顯示原因,加 `--yes` 才執行

## 說明對照表
- 兩張表:`HINTS`(依指令名稱 / 執行檔檔名)與 `ARG_HINTS`(依指令列,辨識 Vite、Next.js、uvicorn、artisan 等)
- 比對規則:先符合者優先,具體的放前面;key 以 `=` 開頭表示完全相等(避免 `ssh` 誤中 `sshd`、`line` 誤中 `pipeline`)
- 指令列辨識只套用在語言執行環境(`RUNTIME_KEYS`:php、python、node、deno、bun、dotnet、java、ruby);資料庫、App、系統服務維持專屬說明,不被指令列中的字眼蓋過
- 指令列單字以空白、`/`、`:` 切開做完全相等比對;`~` 開頭的 key 改為字串包含(用於 Java 這類類別名稱很長的情況)
- 新增項目時:較通用的 key(如 php、python)要放在更具體的 App 之後,例如 `devsense`(VS Code PHP 擴充)必須在 `php` 之前

## 自訂說明
- 規則欄位:`id`、`kind`(cwd / args / command)、`pattern`、`desc`、`advice`、`note`;存成 `{ "rules": [...] }`
- 比對:cwd 以路徑邊界做前綴比對(`/a/ai` 不會命中 `/a/ai-trader`);args / command 不分大小寫的字串包含;多條命中取 pattern 最長者
- 優先順序:自訂 > 內建;但 `assess` 的安全規則(他人進程、系統路徑)仍最優先
- 驗證集中在核心層:比對內容 2~300 字、cwd 必須是絕對路徑且不可為根目錄、說明必填;桌面版與 CLI 共用同一套
- 寫檔先寫暫存檔再改名;設定檔損壞時查詢退回內建說明,管理介面 / `hint list` 才顯示錯誤
- 表單預設:工作目錄具體(非家目錄、根目錄、系統資料夾)用 cwd,否則用程式名稱(`cwd_generic`)

## 視窗啟動行為
- 選單列程式沒有 Dock 圖示,啟動時必須主動 `show` + `set_focus`,並處理 `RunEvent::Reopen`(再次點擊 App 圖示),否則視窗會被擋在其他視窗後面
