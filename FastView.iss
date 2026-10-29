[Setup]
AppId={{0ACC1580-1FCC-4B48-8590-C2FE5DCA1C06}
AppName=FastView
AppVersion=1.1.0
AppVerName=FastView 1.1.0
AppPublisher=Md. Safwan
AppPublisherURL=https://github.com/sayedmdsafwan/fastview
AppSupportURL=https://github.com/sayedmdsafwan/fastview/issues
DefaultDirName={autopf}\FastView
DefaultGroupName=FastView
OutputDir=installer
OutputBaseFilename=FastView_Setup
SetupIconFile=icon.ico
UninstallDisplayIcon={app}\fastview.exe
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
DisableProgramGroupPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
CloseApplications=no
AllowNoIcons=yes
ChangesAssociations=yes

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked

[Files]
Source: "target\release\fastview.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\FastView"; Filename: "{app}\fastview.exe"
Name: "{group}\Uninstall FastView"; Filename: "{uninstallexe}"
Name: "{autodesktop}\FastView"; Filename: "{app}\fastview.exe"; Tasks: desktopicon

[Registry]
; Per-user registration, no admin needed. This does not change the default
; image app; it only makes FastView appear in Explorer's "Open with" list.

; ProgID used by the extension list.
Root: HKCU; Subkey: "Software\Classes\FastView.Image"; ValueType: string; ValueData: "FastView picture"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\FastView.Image\DefaultIcon"; ValueType: string; ValueData: """{app}\fastview.exe"",0"
Root: HKCU; Subkey: "Software\Classes\FastView.Image\shell\open\command"; ValueType: string; ValueData: """{app}\fastview.exe"" ""%1"""

; Application entry: what Windows 11 reads for the "Open with" list.
Root: HKCU; Subkey: "Software\Classes\Applications\fastview.exe"; ValueType: string; ValueData: "FastView"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\Applications\fastview.exe"; ValueType: string; ValueName: "FriendlyAppName"; ValueData: "FastView"
Root: HKCU; Subkey: "Software\Classes\Applications\fastview.exe"; ValueType: string; ValueName: "ApplicationCompany"; ValueData: "Md. Safwan"
Root: HKCU; Subkey: "Software\Classes\Applications\fastview.exe\DefaultIcon"; ValueType: string; ValueData: """{app}\fastview.exe"",0"
Root: HKCU; Subkey: "Software\Classes\Applications\fastview.exe\shell\open\command"; ValueType: string; ValueData: """{app}\fastview.exe"" ""%1"""
Root: HKCU; Subkey: "Software\Classes\Applications\fastview.exe\SupportedTypes"; ValueType: string; ValueName: ".jpg"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\fastview.exe\SupportedTypes"; ValueType: string; ValueName: ".jpeg"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\fastview.exe\SupportedTypes"; ValueType: string; ValueName: ".jpe"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\fastview.exe\SupportedTypes"; ValueType: string; ValueName: ".jfif"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\fastview.exe\SupportedTypes"; ValueType: string; ValueName: ".png"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\fastview.exe\SupportedTypes"; ValueType: string; ValueName: ".webp"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\fastview.exe\SupportedTypes"; ValueType: string; ValueName: ".gif"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\fastview.exe\SupportedTypes"; ValueType: string; ValueName: ".bmp"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\fastview.exe\SupportedTypes"; ValueType: string; ValueName: ".tif"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\fastview.exe\SupportedTypes"; ValueType: string; ValueName: ".tiff"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\fastview.exe\SupportedTypes"; ValueType: string; ValueName: ".ico"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\fastview.exe\SupportedTypes"; ValueType: string; ValueName: ".heic"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\fastview.exe\SupportedTypes"; ValueType: string; ValueName: ".heif"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\fastview.exe\SupportedTypes"; ValueType: string; ValueName: ".avif"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\fastview.exe\SupportedTypes"; ValueType: string; ValueName: ".jxl"; ValueData: ""

; Settings > Default apps entry.
Root: HKCU; Subkey: "Software\Classes\FastView"; ValueType: none; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\FastView\Capabilities"; ValueType: string; ValueName: "ApplicationName"; ValueData: "FastView"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\FastView\Capabilities"; ValueType: string; ValueName: "ApplicationDescription"; ValueData: "A fast, tiny photo viewer for Windows"
Root: HKCU; Subkey: "Software\Classes\FastView\Capabilities"; ValueType: string; ValueName: "ApplicationIcon"; ValueData: """{app}\fastview.exe"",0"
Root: HKCU; Subkey: "Software\Classes\FastView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".jpg"; ValueData: "FastView.Image"
Root: HKCU; Subkey: "Software\Classes\FastView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".jpeg"; ValueData: "FastView.Image"
Root: HKCU; Subkey: "Software\Classes\FastView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".png"; ValueData: "FastView.Image"
Root: HKCU; Subkey: "Software\Classes\FastView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".webp"; ValueData: "FastView.Image"
Root: HKCU; Subkey: "Software\Classes\FastView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".gif"; ValueData: "FastView.Image"
Root: HKCU; Subkey: "Software\Classes\FastView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".bmp"; ValueData: "FastView.Image"
Root: HKCU; Subkey: "Software\Classes\FastView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".tif"; ValueData: "FastView.Image"
Root: HKCU; Subkey: "Software\Classes\FastView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".tiff"; ValueData: "FastView.Image"
Root: HKCU; Subkey: "Software\Classes\FastView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".jpe"; ValueData: "FastView.Image"
Root: HKCU; Subkey: "Software\Classes\FastView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".jfif"; ValueData: "FastView.Image"
Root: HKCU; Subkey: "Software\Classes\FastView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".ico"; ValueData: "FastView.Image"
Root: HKCU; Subkey: "Software\Classes\FastView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".heic"; ValueData: "FastView.Image"
Root: HKCU; Subkey: "Software\Classes\FastView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".heif"; ValueData: "FastView.Image"
Root: HKCU; Subkey: "Software\Classes\FastView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".avif"; ValueData: "FastView.Image"
Root: HKCU; Subkey: "Software\Classes\FastView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".jxl"; ValueData: "FastView.Image"
Root: HKCU; Subkey: "Software\RegisteredApplications"; ValueType: string; ValueName: "FastView"; ValueData: "Software\Classes\FastView\Capabilities"; Flags: uninsdeletevalue

; One OpenWithProgids value per extension.
Root: HKCU; Subkey: "Software\Classes\.jpg\OpenWithProgids"; ValueType: string; ValueName: "FastView.Image"; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.jpeg\OpenWithProgids"; ValueType: string; ValueName: "FastView.Image"; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.jpe\OpenWithProgids"; ValueType: string; ValueName: "FastView.Image"; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.jfif\OpenWithProgids"; ValueType: string; ValueName: "FastView.Image"; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.png\OpenWithProgids"; ValueType: string; ValueName: "FastView.Image"; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.webp\OpenWithProgids"; ValueType: string; ValueName: "FastView.Image"; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.gif\OpenWithProgids"; ValueType: string; ValueName: "FastView.Image"; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.bmp\OpenWithProgids"; ValueType: string; ValueName: "FastView.Image"; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.tif\OpenWithProgids"; ValueType: string; ValueName: "FastView.Image"; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.tiff\OpenWithProgids"; ValueType: string; ValueName: "FastView.Image"; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.ico\OpenWithProgids"; ValueType: string; ValueName: "FastView.Image"; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.heic\OpenWithProgids"; ValueType: string; ValueName: "FastView.Image"; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.heif\OpenWithProgids"; ValueType: string; ValueName: "FastView.Image"; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.avif\OpenWithProgids"; ValueType: string; ValueName: "FastView.Image"; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.jxl\OpenWithProgids"; ValueType: string; ValueName: "FastView.Image"; ValueData: ""; Flags: uninsdeletevalue

; Marker used by the app's own registration; removed with the app.
Root: HKCU; Subkey: "Software\FastView"; ValueType: string; ValueName: "OpenWithExe"; ValueData: "{app}\fastview.exe"; Flags: uninsdeletekey

[Run]
Filename: "{app}\fastview.exe"; Parameters: "--set-default"; Description: "Set FastView as default photo viewer (opens Windows Settings)"; Flags: nowait postinstall skipifsilent
Filename: "{app}\fastview.exe"; Description: "Launch FastView"; Flags: nowait postinstall skipifsilent
