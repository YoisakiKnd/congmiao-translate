!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegKey HKCU "Software\Classes\congmiao"
  DeleteRegKey HKCU "Software\Google\Chrome\NativeMessagingHosts\app.congmiao.translate"
!macroend
