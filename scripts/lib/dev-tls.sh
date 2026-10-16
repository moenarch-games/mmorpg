# shellcheck shell=bash
# Disposable development TLS credentials shared by the local launchers.
#
# Source this file; it only defines functions. Certificates are ECDSA P-256
# and valid for 10 days, so browsers accept them through WebTransport
# certificate hashes (which require at most 14 days) and the native client
# through --certificate. Keep them under target/dev-tls/, which git ignores.

# Whether CERT and KEY form a reusable development pair: valid for at least
# another day, expiring within 14 days, an ECDSA P-256 key, and matching.
dev_certificate_is_valid() {
  local certificate=$1 key=$2 lan_address=${3:-}
  [[ -f "$certificate" && -f "$key" ]] \
    && openssl x509 -checkend 86400 -noout -in "$certificate" >/dev/null 2>&1 \
    && ! openssl x509 -checkend 1209600 -noout -in "$certificate" >/dev/null 2>&1 \
    && { [[ -z "$lan_address" ]] || openssl x509 -in "$certificate" -checkip "$lan_address" -noout >/dev/null 2>&1; } \
    && openssl pkey -in "$key" -noout -text 2>/dev/null | grep -q 'prime256v1' \
    && cmp -s \
      <(openssl x509 -in "$certificate" -pubkey -noout | openssl pkey -pubin -outform DER) \
      <(openssl pkey -in "$key" -pubout -outform DER)
}

# Writes a new pair into DIR/cert.pem and DIR/key.pem, replacing both only
# after the new pair was generated.
generate_dev_certificate() {
  local directory=$1 lan_address=${2:-} temporary
  local subject_alt_name='DNS:localhost,IP:127.0.0.1'
  if [[ -n "$lan_address" ]]; then
    subject_alt_name+=",IP:$lan_address"
  fi
  temporary="$(mktemp -d "$directory/.new.XXXXXX")"
  if ! (
    umask 077
    openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 \
      -keyout "$temporary/key.pem" \
      -out "$temporary/cert.pem" \
      -sha256 -days 10 -nodes \
      -subj /CN=localhost \
      -addext "subjectAltName=$subject_alt_name" \
      >/dev/null 2>&1
  ) || ! mv -f -- "$temporary/cert.pem" "$directory/cert.pem" \
    || ! mv -f -- "$temporary/key.pem" "$directory/key.pem"; then
    rm -rf -- "$temporary"
    return 1
  fi
  rmdir -- "$temporary"
}

# Reuses a valid pair in DIR or generates a new one, and keeps the key private.
ensure_dev_certificate() {
  local directory=$1 lan_address=${2:-}
  mkdir -p "$directory"
  if ! dev_certificate_is_valid "$directory/cert.pem" "$directory/key.pem" "$lan_address"; then
    generate_dev_certificate "$directory" "$lan_address"
  fi
  chmod 600 "$directory/key.pem"
}

# The base64 SHA-256 digest of CERT's DER encoding: a WebTransport
# serverCertificateHashes value.
dev_certificate_hash() {
  openssl x509 -in "$1" -outform DER | openssl dgst -sha256 -binary | openssl base64 -A
}
