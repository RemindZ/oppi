@echo off
setlocal

pushd "%~dp0.." >nul
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0rust-dev-startup.ps1" %*
set "OPPI_EXIT=%ERRORLEVEL%"
popd >nul

echo.
echo OPPi Rust TUI exited with code %OPPI_EXIT%.
pause
exit /b %OPPI_EXIT%
