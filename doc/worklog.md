# 工作日誌

## 2026-10-28(發佈到 GitHub)
- 建立公開 repo https://github.com/qscgy5713/lazyport,remote 用 SSH;`target/`、`node_modules/` 等產物不上傳
- README 安裝步驟改成 `git clone`(用 HTTPS 網址,未設定 SSH 金鑰的人也能下載)

## 2026-10-28(自訂說明 + 修正「打開後什麼都沒有」)
- 使用者回報「打開之後沒有任何東西」,實際有兩個原因:
  1. 視窗被擋在其他視窗後面:選單列程式沒有 Dock 圖示,啟動時沒有前置 → 啟動時 `show_window`,並處理 `RunEvent::Reopen`
  2. 表格是空的:做「加強警告」時,字串替換範圍太大,不小心刪掉 `rowKey` 函式,每一列渲染都丟 ReferenceError。編譯不會報錯,而當時只確認「App 啟動 4 秒沒崩潰」,沒有看畫面,所以沒發現
- 除錯過程:先排除 PATH / 環境差異(空環境與 launchd 下 lsof 正常),再加暫時記錄確認後端回傳 15 筆,才定位到前端;記錄已移除
- 新增自訂說明:`src/custom.rs`(核心)、Tauri 三個指令、桌面版表單 / 管理清單、CLI `hint list|add|rm`
- 驗證方式改為「真的看畫面」:用假的 Tauri IPC 在瀏覽器實際點擊,確認新增、空說明被擋、儲存後標籤與紅色「不建議關閉」、按結束出現加強警告、刪除後回到內建說明、主控台無錯誤;再用真實程式測 CLI(自訂優先、kill 被拒絕、輸入驗證、設定檔損壞時不影響查詢);最後截圖確認真實 .app 列表顯示
- `cargo test` 15 項通過
- 教訓:改動前端後要看畫面;大範圍字串替換後要確認沒有誤刪(可在替換前後比對 diff)

## 2026-10-28(擴充說明表)
- 對照表從約 20 種擴充到約 95 種(macOS 系統服務、常用 App、資料庫 / 基礎設施、VPN / 隧道、容器 / 虛擬機、各語言執行環境),新增 46 種「依指令列辨識」的開發工具
- 用本機實際 port 驗證,發現並修正:`devsense.php.ls`(VS Code PHP 語言伺服器)因含 `php` 被誤判為 PHP 開發伺服器;Postman、LINE 原本沒有說明
- code review:原本靠說明文字是否含「看「」「通常是」來判斷哪些可被指令列覆蓋,太脆弱 → 改為明確的 `RUNTIME_KEYS`
- 驗證:`cargo test` 9 項通過、.app 重新打包並啟動 4 秒不崩潰

## 2026-10-28(加強警告)
- 桌面版:對「不建議關閉」的程式,按結束改展開紅色警告列,需明確按「我了解風險,仍要結束」;CLI 需 `--yes`
- code review:發現警告判斷若用快取可能因 pid 被重新分配而誤判 → 每次重新查詢;查詢期間停用按鈕避免連點繞過警告
- 驗證:CLI 對 rapportd(port 49206)拒絕結束,進程仍在;桌面版僅通過前端建置與打包,未實測畫面

## 2026-10-28(修正展開錯誤)
- 問題:點開一個 com.docker 的 port,所有 com.docker 的列都跟著展開。原因:展開狀態只用 pid 判斷,而同一個 pid 會開很多 port
- 修正:展開狀態改用 `port-pid`;已重新打包 .app,未實測畫面

## 2026-10-28(關閉建議)
- 展開詳情新增「建議關閉嗎 / 關閉有沒有問題」:`Advice` 三級加影響說明,CLI `info` 也會顯示
- 安全規則優先:別的使用者、macOS 系統路徑一律 Avoid;HINTS 每筆補上等級與影響
- 驗證:`cargo test` 5 項通過、`info` 對 adb(可以關閉)/ Docker(留意)/ Python(可以關閉)輸出正確、.app 啟動 4 秒不崩潰;未實測桌面版展開畫面

## 2026-10-28(程式說明、單色圖示、自訂快捷鍵)
- 需求:看不懂 port 後面是什麼程式 → 新增 `process_info` 與內建說明對照表,CLI 加 `info`,桌面版可展開詳情
- 選單列改單色 template 圖示(`icons/tray.png`,`icon_as_template`)
- 快捷鍵可自訂並存檔
- code review:說明比對改為只比對執行檔檔名(原本比對完整路徑,`python-projects` 之類資料夾會誤判),補測試
- 驗證:`cargo test` 4 項通過、前端建置與 .app 打包成功、.app 啟動 4 秒不崩潰;未實測:展開詳情、錄製快捷鍵、深色選單列圖示(需人工操作)

## 2026-10-28(打包)
- `tauri build --bundles app` 產出 lazyport.app(10.26 MiB),直接執行 4 秒不崩潰
- 未簽署,Gatekeeper 首次需右鍵開啟

## 2026-10-28(選單列)
- 桌面版加入選單列常駐、左鍵切換視窗、右鍵選單、全域快捷鍵 ⌘⇧P
- code review:`set_activation_policy` 加 macOS 條件編譯;快捷鍵註冊失敗不讓 App 崩潰
- 驗證:編譯成功、啟動 5 秒不崩潰;未實測圖示點擊與快捷鍵(需人工操作)

## 2026-10-28(續)
- 使用者指出原先只做了 CLI,實際想要桌面應用 → 保留 CLI,新增桌面版
- 核心拆成 `src/lib.rs`;新增 `desktop/`(Tauri 2 + Svelte 5)
- 驗證:`cargo test` 3 項通過、`npm run build` 成功、`cargo build` 成功、桌面程式啟動 5 秒不崩潰
- code review:結束進程只接受 port,由核心層重查 pid,避免前端指定任意 pid;移除用不到的 android/ios 圖示
- 尚未實測:在視窗內實際按「結束」(避免誤殺使用中的服務)

## 2026-10-28
- 建立 lazyport 專案(Rust,零依賴)
- 完成列表、查詢、kill 功能與 3 個單元測試
- code review:修正 `kill -f 3000`(旗標在前)會被誤判為 port 的問題
