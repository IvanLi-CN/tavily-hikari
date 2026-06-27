#!/bin/sh
set -eu

date +%s > /tmp/tavily-hikari-started-at
exec tavily-hikari "$@"
