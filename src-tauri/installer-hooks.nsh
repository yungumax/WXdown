; 安装器自定义钩子（tauri.conf.json → bundle.windows.nsis.installerHooks）
;
; 为什么需要它：Tauri 卸载器「删除应用数据」勾选后只删 $APPDATA\<bundle id>，
; 也就是 %APPDATA%\com.wxdown.app；而本软件的真实数据目录是 %APPDATA%\wxdown
; （登录态 webview、settings.json、wx_debug.log 都在那里）——不补这一钩子，
; 卸载时勾了"删除应用数据"也会留下一整份数据。
;
; NSIS_HOOK_POSTUNINSTALL 由 Tauri 模板在 Uninstall 区段的清数据步骤之后插入，
; 此时 $DeleteAppDataCheckboxState == 1 表示用户勾了"删除应用数据"。

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
    ; 软件真实数据目录（与 state.rs 的 default_data_root() 保持一致）
    RmDir /r "$APPDATA\wxdown"
    RmDir /r "$LOCALAPPDATA\wxdown"
  ${EndIf}
!macroend
