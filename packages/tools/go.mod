module github.com/astral-app/astral/packages/tools

go 1.27

require (
	github.com/gorilla/websocket v1.5.3
	github.com/pelletier/go-toml/v2 v2.4.3
	golang.org/x/crypto v0.59.0
	howett.net/plist v1.0.1
)

require (
	github.com/bmatcuk/doublestar/v4 v4.10.0 // indirect
	github.com/clipperhouse/uax29/v2 v2.7.0 // indirect
	github.com/fatih/color v1.19.0 // indirect
	github.com/google/go-cmp v0.7.0 // indirect
	github.com/google/yamlfmt v0.21.0 // indirect
	github.com/mattn/go-colorable v0.1.14 // indirect
	github.com/mattn/go-isatty v0.0.20 // indirect
	github.com/mattn/go-runewidth v0.0.21 // indirect
	github.com/mattn/go-shellwords v1.0.12 // indirect
	github.com/mitchellh/mapstructure v1.5.0 // indirect
	github.com/rhysd/actionlint v1.7.12 // indirect
	github.com/robfig/cron/v3 v3.0.1 // indirect
	github.com/sabhiram/go-gitignore v0.0.0-20210923224102-525f6e181f06 // indirect
	go.yaml.in/yaml/v3 v3.0.3 // indirect
	go.yaml.in/yaml/v4 v4.0.0-rc.3 // indirect
	golang.org/x/sync v0.20.0 // indirect
	golang.org/x/sys v0.50.0 // indirect
	sigs.k8s.io/yaml v1.6.0 // indirect
)

tool (
	github.com/google/yamlfmt/cmd/yamlfmt
	github.com/rhysd/actionlint/cmd/actionlint
)
