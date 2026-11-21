#!/bin/sh
# One-time: create a self-signed code-signing identity "myCommander Release Signing" in the
# login keychain (and a backup in ~/.myCommander-signing/). Releases signed with it keep their
# identity across versions, so macOS keeps the privacy grants the person gave the app.
# Keep the backup: with a new certificate the grants have to be given once more.
set -eu
ID="myCommander Release Signing"
security find-identity -p codesigning | grep -q "$ID" && { echo "identity already exists"; exit 0; }
D="$HOME/.myCommander-signing"; mkdir -p "$D"; chmod 700 "$D"; cd "$D"
cat > cert.cnf <<EOF
[req]
distinguished_name=dn
x509_extensions=ext
prompt=no
[dn]
CN=$ID
[ext]
basicConstraints=critical,CA:false
keyUsage=critical,digitalSignature
extendedKeyUsage=critical,codeSigning
EOF
/usr/bin/openssl req -x509 -newkey rsa:2048 -nodes -keyout key.pem -out cert.pem -days 3650 -config cert.cnf
PW=$(/usr/bin/openssl rand -hex 12); echo "$PW" > p12.pass
/usr/bin/openssl pkcs12 -export -inkey key.pem -in cert.pem -out id.p12 -passout pass:"$PW" -name "$ID"
chmod 600 key.pem id.p12 p12.pass
security import id.p12 -k "$HOME/Library/Keychains/login.keychain-db" -P "$PW" -T /usr/bin/codesign
