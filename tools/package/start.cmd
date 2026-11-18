@echo off
rem Starts the SS14 map editor from its unzipped package. It opens on the start screen, where
rem you choose a fork. It needs the .NET 10 runtime, and the .NET 10 SDK to build an engine side
rem for a fork's code the first time you open that fork. The SDK includes the runtime
rem (https://dotnet.microsoft.com/download).
setlocal
set "HERE=%~dp0"
set "SS14_EDITOR_ENGINE=%HERE%engine"

"%HERE%ss14-editor.exe" "%SS14_EDITOR_ENGINE%"
if errorlevel 1 pause
