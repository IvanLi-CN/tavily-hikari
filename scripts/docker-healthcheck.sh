#!/bin/sh
set -eu

started_at_file=/tmp/tavily-hikari-started-at

[ -s "$started_at_file" ]

started_at="$(cat "$started_at_file")"
now="$(date +%s)"
[ $((now - started_at)) -ge 20 ]

curl --fail --silent http://127.0.0.1:8787/health >/dev/null
