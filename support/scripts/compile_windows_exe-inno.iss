; HyX Windows 安装包 Inno Setup 脚本
; 参照 LocalSend 的 compile_windows_exe-inno.iss
; 使用方法（本地）：
;   1. 将 Flutter Windows 构建产物（app/build/windows/x64/runner/Release/*）复制到 D:\inno
;   2. 运行: iscc support\scripts\compile_windows_exe-inno.iss
; 使用方法（CI）：
;   iscc "/DPayloadDir=<目录>" "/DResultDir=<目录>" "/DMyAppVersion=<版本>" support\scripts\compile_windows_exe-inno.iss

; Payload/output 目录，CI 可通过 /DPayloadDir=... /DResultDir=... 覆盖
#ifndef PayloadDir
  #define PayloadDir "D:\inno"
#endif
#ifndef ResultDir
  #define ResultDir "D:\inno-result"
#endif

; 版本号，CI 可通过 /DMyAppVersion=... 覆盖；需与 app/pubspec.yaml 保持一致
#ifndef MyAppVersion
  #define MyAppVersion "1.0.0"
#endif

#define MyAppName "HyX"
#define MyAppPublisher "ojbkxc"
#define MyAppURL "https://github.com/ojbkxc/HyX"
#define MyAppExeName "hyx_app.exe"

[Setup]
; NOTE: AppId 唯一标识此应用程序，不要在其他应用的安装包中复用此值
AppId={{A1B2C3D4-E5F6-4A5B-9C7D-EF1234567890}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}
DefaultDirName={autopf}\{#MyAppName}
DisableProgramGroupPage=yes
; 取消注释以下行可改为非管理员安装模式（仅当前用户）
;PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
OutputDir={#ResultDir}
OutputBaseFilename=HyX
; 可选自定义图标：CI 传 /DLogoFile=<路径> 时启用，否则使用 Inno Setup 默认图标
#ifdef LogoFile
  SetupIconFile={#LogoFile}
#endif
UninstallDisplayIcon={app}\{#MyAppExeName}
Compression=lzma
SolidCompression=yes
WizardStyle=modern
ArchitecturesInstallIn64BitMode=x64compatible
ArchitecturesAllowed=x64compatible

[Languages]

Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
; 主可执行文件
Source: "{#PayloadDir}\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion
; 所有 DLL
Source: "{#PayloadDir}\*.dll"; DestDir: "{app}"; Flags: ignoreversion
; data 目录（Flutter 资源）
Source: "{#PayloadDir}\data\*"; DestDir: "{app}\data"; Flags: ignoreversion recursesubdirs createallsubdirs
; NOTE: 不要对共享系统文件使用 "Flags: ignoreversion"

[Icons]
; 开始菜单快捷方式
Name: "{autoprograms}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
; 桌面快捷方式（可选）
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Run]
; 安装完成后启动应用
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent