package main

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"net"
	"os"
	"os/exec"
	"strings"
	"time"

	"golang.org/x/crypto/ssh"
)

const (
	vphoneUser     = "mobile"
	vphonePassword = "alpine"
)

func vphoneSSHAddress(name string) (string, error) {
	if address := os.Getenv("VPHONE_SSH"); address != "" {
		return address, nil
	}
	raw, err := exec.Command("vphone-cli", "vm", "leases", "--json").Output()
	if err != nil {
		return "", fmt.Errorf("vphone-cli vm leases: %w", err)
	}
	var leases []struct {
		Address string `json:"address"`
		Machine string `json:"machine"`
	}
	if err := json.Unmarshal(raw, &leases); err != nil {
		return "", err
	}
	for _, lease := range leases {
		if lease.Machine == name && lease.Address != "" {
			return net.JoinHostPort(lease.Address, "22"), nil
		}
	}
	return "", fmt.Errorf("%s has no DHCP lease yet; is it booted? (or set VPHONE_SSH=host:port)", name)
}

func vphoneSSH(name string) (*ssh.Client, error) {
	address, err := vphoneSSHAddress(name)
	if err != nil {
		return nil, err
	}
	config := &ssh.ClientConfig{
		User:            vphoneUser,
		Auth:            []ssh.AuthMethod{ssh.Password(vphonePassword)},
		HostKeyCallback: ssh.InsecureIgnoreHostKey(),
		Timeout:         10 * time.Second,
	}
	var client *ssh.Client
	for attempt := 0; attempt < 10; attempt++ {
		if client, err = ssh.Dial("tcp", address, config); err == nil {
			break
		}
		time.Sleep(2 * time.Second)
	}
	if err != nil {
		return nil, fmt.Errorf("ssh %s@%s: %w (is owngoal-bootstrap-vphone installed?)", vphoneUser, address, err)
	}
	return client, nil
}

func vphoneRun(name string, command string, root bool) (string, error) {
	client, err := vphoneSSH(name)
	if err != nil {
		return "", err
	}
	defer client.Close()
	session, err := client.NewSession()
	if err != nil {
		return "", err
	}
	defer session.Close()
	if root {
		command = "echo " + vphonePassword + " | sudo -S -p '' sh -c " + shellQuote(command)
	}
	var output bytes.Buffer
	session.Stdout = &output
	session.Stderr = &output
	err = session.Run(command)
	return output.String(), err
}

func shellQuote(text string) string {
	return "'" + strings.ReplaceAll(text, "'", `'\''`) + "'"
}

func runVphoneSSHCommand(args []string) error {
	flags := newFlags("vphone ssh")
	name := flags.String("name", "vphone", "VM name")
	root := flags.Bool("root", false, "run the command as root through sudo")
	if err := flags.Parse(args); err != nil {
		return err
	}
	if flags.NArg() == 0 {
		return errors.New("usage: astral vphone ssh [--root] [--name vphone] <command>")
	}
	output, err := vphoneRun(*name, strings.Join(flags.Args(), " "), *root)
	fmt.Print(output)
	return err
}

const ellekitRepository = "https://apt.owngoal.dev"

func runVphoneSetup(args []string) error {
	flags := newFlags("vphone setup")
	name := flags.String("name", "vphone", "VM name")
	if err := flags.Parse(args); err != nil {
		return err
	}
	client, err := newVphoneClient()
	if err != nil {
		return err
	}
	if _, err := vphoneRun(*name, "test -x /usr/bin/irisin-cli", false); err != nil {
		return fmt.Errorf("the OwnGoal bootstrap is missing; create the VM with vphone-cli vm create --bootstrap roothide: %w", err)
	}
	if _, err := vphoneRun(*name, "test -f /usr/lib/TweakLoader.dylib", false); err != nil {
		output, err := vphoneRun(*name, "irisin-cli install -y --repository "+ellekitRepository+" ellekit", true)
		if err != nil {
			return fmt.Errorf("installing ElleKit failed: %w\n%s", err, output)
		}
		fmt.Println("installed ElleKit from " + ellekitRepository + "; restarting the VM")
		if err := bootVphone(*name); err != nil {
			return err
		}
	}
	for attempt := 0; attempt < 30; attempt++ {
		if output, err := vphoneRun(*name, "echo injected-shell-ok", false); err == nil && strings.Contains(output, "injected-shell-ok") {
			fmt.Println("ElleKit is injecting and bootstrap shells still run")
			if err := disableAutoLock(client, *name); err != nil {
				return err
			}
			fmt.Println("turned auto-lock off")
			return nil
		}
		time.Sleep(3 * time.Second)
	}
	return errors.New("shells die under ElleKit; the VM needs the txm-exp-precheck_admission patch (see the vphone skill)")
}
