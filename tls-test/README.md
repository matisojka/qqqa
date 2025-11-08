# TLS mock server quickstart

```sh
cd tls-mock
python -m venv .venv && source .venv/bin/activate
pip install fastapi "uvicorn[standard]"
```

Always install deps inside the virtual environment above; other installation methods tend to cause path/resolution issues. Quoting `"uvicorn[standard]"` avoids zsh globbing errors.

Generate a self-signed cert (mkcert example):

```sh
mkdir -p ~/.qq/certs
mkcert -key-file ~/.qq/certs/mock.key -cert-file ~/.qq/certs/mock.pem localhost 127.0.0.1
cp "$(mkcert -CAROOT)/rootCA.pem" ~/.qq/certs/mock-ca.pem
```

Run the server with TLS:

```sh
uvicorn mock_server:app \
  --host 127.0.0.1 --port 8443 \
  --ssl-certfile ~/.qq/certs/mock.pem \
  --ssl-keyfile ~/.qq/certs/mock.key
```

Add a provider profile in `~/.qq/config.json`:

```json
"model_providers": {
  "mock": {
    "name": "Mock HTTPS",
    "base_url": "https://127.0.0.1:8443",
    "env_key": "MOCK_API_KEY",
    "local": true,
    "tls": { "ca_bundle_path": "certs/mock-ca.pem" }
  }
},
"profiles": {
  "mock": { "model_provider": "mock", "model": "fake-model" }
}
```

Test the certificate handling:

```sh
export MOCK_API_KEY=local
qq --profile mock "ping"
```

You should see the mock response. Remove the `tls` block and rerun to reproduce the `UnknownIssuer` error, confirming the feature works.
