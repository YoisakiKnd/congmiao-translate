#!/bin/sh
# 给开发版使用固定的本机签名身份，避免每次重新编译后辅助功能和屏幕录制授权丢失。
# 这个脚本不会把证书写进登录钥匙串，以免弹出系统授权。
set -eu

name="Congmiao Dev"
app="${1:-apps/desktop/src-tauri/target/debug/bundle/macos/Congmiao Translate.app}"

if ! security find-identity -p codesigning | grep -q "$name"; then
  echo "没有找到名为「${name}」的代码签名身份。"
  echo "请在钥匙串访问中自行创建并信任这个自签名代码签名证书，然后重新运行本脚本。"
  echo "不要用临时的 ad-hoc 签名（codesign -s -），那会在每次编译后换掉 cdhash，系统授权会失效。"
  exit 0
fi

if [ ! -d "$app" ]; then
  echo "找不到应用：$app"
  echo "先编译桌面端，再把 .app 路径传给本脚本。"
  exit 1
fi

codesign --force --sign "$name" --deep "$app"
echo "已用「${name}」签名：$app"
