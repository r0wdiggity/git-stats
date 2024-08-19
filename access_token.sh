#!/usr/bin/env bash

set -o pipefail

app_id=$1 # App ID as first argument
pem=$( cat $2 ) # file path of the private key as second argument

now=$(date +%s)
iat=$((${now} - 60)) # Issues 60 seconds in the past
exp=$((${now} + 600)) # Expires 10 minutes in the future

b64enc() { openssl base64 | tr -d '=' | tr '/+' '_-' | tr -d '\n'; }

header_json='{
    "typ":"JWT",
    "alg":"RS256"
}'
# Header encode
header=$( echo -n "${header_json}" | b64enc )

payload_json='{
    "iat":'"${iat}"',
    "exp":'"${exp}"',
    "iss":'"${app_id}"'
}'
# Payload encode
payload=$( echo -n "${payload_json}" | b64enc )

# Signature
header_payload="${header}"."${payload}"
signature=$(
    openssl dgst -sha256 -sign <(echo -n "${pem}") \
    <(echo -n "${header_payload}") | b64enc
)

# Create JWT
JWT="${header_payload}"."${signature}"

#use the JWT token to get installations (the id#
installid=$(curl -s -i -H "Authorization: Bearer $JWT" -H "Accept: application/vnd.github.v3+json" https://api.github.com/app/installations | grep html_url | grep installations | cut -d '/' -f 8 | cut -d '"' -f1)
#Now get an ACCESS TOKEN
accesstoken=$(curl -s -i -X POST -H "Authorization: Bearer $JWT" -H "Accept: application/vnd.github.v3+json" https://api.github.com/app/installations/$installid/access_tokens | grep token | cut -d '"' -f 4)

export GITHUB_TOKEN=$accesstoken

echo "Exported GITHUB_TOKEN"
