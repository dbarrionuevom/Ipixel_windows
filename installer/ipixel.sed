[Version]
Class=IEXPRESS
SEDVersion=3
[Options]
PackagePurpose=InstallApp
ShowInstallProgramWindow=0
HideExtractAnimation=0
UseLongFileName=1
InsideCompressed=0
CAB_FixedSize=0
CAB_ResvCodeSigning=0
RebootMode=N
InstallPrompt=%InstallPrompt%
DisplayLicense=%DisplayLicense%
FinishMessage=%FinishMessage%
TargetName=%TargetName%
FriendlyName=%FriendlyName%
AppLaunched=%AppLaunched%
PostInstallCmd=%PostInstallCmd%
AdminQuietInstCmd=%AdminQuietInstCmd%
UserQuietInstCmd=%UserQuietInstCmd%
SourceFiles=SourceFiles
[Strings]
InstallPrompt=¿Deseas instalar iPixel 32x32 Controller en este equipo?
DisplayLicense=
FinishMessage=
TargetName=f:\Proyectos IA\Pantalla led matricial\Ipixel_windows\iPixel_32x32_Setup.exe
FriendlyName=iPixel 32x32 Controller Setup
AppLaunched=cmd.exe /c powershell -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File install.ps1
PostInstallCmd=<None>
AdminQuietInstCmd=
UserQuietInstCmd=
FILE0="ipixel_windows.exe"
FILE1="install.ps1"
FILE2="uninstall.ps1"
[SourceFiles]
SourceFiles0=f:\Proyectos IA\Pantalla led matricial\Ipixel_windows\target\release\
SourceFiles1=f:\Proyectos IA\Pantalla led matricial\Ipixel_windows\installer\
[SourceFiles0]
%FILE0%=
[SourceFiles1]
%FILE1%=
%FILE2%=
