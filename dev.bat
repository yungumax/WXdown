@echo off
chcp 65001 >nul
cd /d "%~dp0"

set "WBO=%~dp0"
set "SDK=C:\Program Files (x86)\Windows Kits\10"
set "SDKVER=10.0.26100.0"

rem 便携 MSVC 链接环境：需要 .lldbin（本仓库没有就借用 WeBodown 的同款目录），
rem 或者在本机安装 VS Build Tools 后删掉这些 set 行。
if not exist "%WBO%.lldbin" (
    if exist "D:\Zcode\WeBodown\.lldbin" (
        mklink /J "%WBO%.lldbin" "D:\Zcode\WeBodown\.lldbin" >nul 2>nul
    )
)

if exist "%WBO%.lldbin" (
    set "VC_LIB=%WBO%.lldbin\x64_extract\Program Files\Microsoft Visual Studio 14.0\VC\lib\amd64"
    set "VC_INC=%WBO%.lldbin\headers_extract\Program Files\Microsoft Visual Studio 14.0\VC\include"
    set "VC1444_BIN=%WBO%.lldbin\vc1444\Contents\VC\Tools\MSVC\14.44.35207\bin\Hostx64\x64"
    set "RUSTFLAGS=-C linker=%WBO%.lldbin\lld-link.exe -C link-arg=/nodefaultlib:msvcrt -C link-arg=/nodefaultlib:OLDNAMES -C link-arg=libcmt.lib -C link-arg=libucrt.lib -C link-arg=libvcruntime.lib"
    set "LIB=%VC_LIB%;%WBO%.lldbin\crt1444\Contents\VC\Tools\MSVC\14.44.35207\lib\x64;%WBO%.lldbin\stubs;%SDK%\Lib\%SDKVER%\ucrt\x64;%SDK%\Lib\%SDKVER%\um\x64"
    set "CFLAGS_x86_64_pc_windows_msvc=-MT"
    set "CXXFLAGS_x86_64_pc_windows_msvc=-MT"
    set "INCLUDE=%VC_INC%;%SDK%\Include\%SDKVER%\ucrt;%SDK%\Include\%SDKVER%\um;%SDK%\Include\%SDKVER%\shared;%SDK%\Include\%SDKVER%\winrt;%SDK%\Include\%SDKVER%\cppwinrt"
    set "PATH=%WBO%.lldbin;%VC1444_BIN%;%SDK%\bin\%SDKVER%\x64;%PATH%"
)

rem 更新包签名密钥（createUpdaterArtifacts 需要本地签名；CI 发布走 GitHub Secrets）
rem 注意变量名：Tauri CLI 读的是 TAURI_SIGNING_PRIVATE_KEY（**密钥内容**），不是 *_PATH。
rem 用错名字不会报错但会静默不签名——打出来的安装包没有 .sig，更新器无法验证（实测踩过）。
set "TAURI_SIGNING_PRIVATE_KEY_FILE=D:\Zcode\_data\tauri-keys\wxdown.key"
set /p TAURI_SIGNING_PRIVATE_KEY=<"%TAURI_SIGNING_PRIVATE_KEY_FILE%"
set "TAURI_SIGNING_PRIVATE_KEY_PASSWORD="

if /i "%~1"=="build" (
    echo 正在打包安装程序 ...
    call npm run tauri build
) else (
    echo 正在启动开发模式（前端热更新 + 桌面窗口）...
    call npm run tauri dev
)

echo.
echo 命令已结束。
pause >nul
