package main

import (
	"errors"
	"fmt"
	"strings"
)

const openUsage = `open <target>, where target is one of:
  user <user id>                        a profile
  channel <guild id> <channel id> [message id]
  dm [channel id]                       the DM list, or one DM
  settings [section]                    Discord's Settings, or a section such as account, privacy, notifications, voice
  discord://… or https://discord.com/…  any link Discord handles`

func snowflakeArgument(value string) bool {
	if len(value) < 15 || len(value) > 20 {
		return false
	}
	for _, character := range value {
		if character < '0' || character > '9' {
			return false
		}
	}
	return true
}

func discordLink(args []string) (string, error) {
	if len(args) == 0 {
		return "", errors.New(openUsage)
	}
	if strings.HasPrefix(args[0], "discord://") || strings.HasPrefix(args[0], "https://") {
		return args[0], nil
	}
	ids := args[1:]
	for _, id := range ids {
		if !snowflakeArgument(id) && args[0] != "settings" {
			return "", fmt.Errorf("%q is not a Discord id", id)
		}
	}
	switch {
	case args[0] == "user" && len(ids) == 1:
		return "discord://-/users/" + ids[0], nil
	case args[0] == "channel" && (len(ids) == 2 || len(ids) == 3):
		return "discord://-/channels/" + strings.Join(ids, "/"), nil
	case args[0] == "dm" && len(ids) <= 1:
		return strings.TrimSuffix("discord://-/channels/@me/"+strings.Join(ids, "/"), "/"), nil
	case args[0] == "settings" && len(ids) <= 1:
		return strings.TrimSuffix("discord://-/settings/"+strings.Join(ids, "/"), "/"), nil
	}
	return "", errors.New(openUsage)
}
