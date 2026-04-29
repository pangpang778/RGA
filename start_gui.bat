@echo off
cd /d %~dp0
cargo run -- --gui --port 18501
pause
