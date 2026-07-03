#!/usr/bin/env python3
"""
openitro-cli.py - Command-line interface for OpeNitro
Communicates with openitrod via UNIX socket.
"""

import argparse
import json
import socket
import sys

SOCKET_PATH = "/run/openitro.sock"
SOCKET_TIMEOUT = 3


def send_command(cmd: str) -> dict:
    try:
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sock.settimeout(SOCKET_TIMEOUT)
        sock.connect(SOCKET_PATH)
        sock.sendall(cmd.encode("utf-8"))
        resp = sock.recv(8192).decode("utf-8")
        sock.close()
        return json.loads(resp)
    except FileNotFoundError:
        print("Error: Socket not found. Is openitrod running?", file=sys.stderr)
        sys.exit(1)
    except ConnectionRefusedError:
        print("Error: Connection refused. Is openitrod active?", file=sys.stderr)
        sys.exit(1)
    except socket.timeout:
        print("Error: Daemon did not respond in time.", file=sys.stderr)
        sys.exit(1)
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)


def print_status(data: dict):
    model = data.get("model", "Unknown")
    print("=" * 48)
    print(f"  OpeNitro — {model}")
    print("=" * 48)
    print(f"  CPU Temp:      {data.get('cpu_temp', '?'):>5}°C")
    print(f"  GPU Temp:      {data.get('gpu_temp', '?'):>5}°C")
    print(f"  System Temp:   {data.get('sys_temp', '?'):>5}°C")
    print("-" * 48)
    cpu_mode = data.get("cpu_fan_mode", "?").upper()
    gpu_mode = data.get("gpu_fan_mode", "?").upper()
    print(f"  CPU Fan:       {data.get('cpu_rpm', 0):>5} RPM  [{cpu_mode}]"
          f"  (manual val: {data.get('cpu_manual_speed', '?')})")
    print(f"  GPU Fan:       {data.get('gpu_rpm', 0):>5} RPM  [{gpu_mode}]"
          f"  (manual val: {data.get('gpu_manual_speed', '?')})")
    print("-" * 48)
    nitro = data.get("nitro_mode", "?").upper()
    power = "AC Plugged-in" if data.get("power_plugged") else "Battery"
    bat = "80% ENABLED" if data.get("battery_limit_active") else "Disabled (100%)"
    cb = "ENABLED" if data.get("coolboost_active") else "Disabled"
    kb = "ENABLED (30s timeout)" if data.get("kb_backlight_timeout") else "Disabled (Always-on)"
    usb = "ENABLED" if data.get("usb_charge_poweroff") else "Disabled"
    print(f"  Power Mode:    {nitro}")
    print(f"  Power Source:  {power}")
    print(f"  Battery Limit: {bat}")
    print(f"  CoolBoost:     {cb}")
    print(f"  KB Timeout:    {kb}")
    print(f"  USB Off-Chg:   {usb}")
    print("=" * 48)


def main():
    parser = argparse.ArgumentParser(
        description="OpeNitro — Command-Line Utility"
    )
    parser.add_argument(
        "-s", "--status", action="store_true", help="Show current system status"
    )
    parser.add_argument(
        "-j", "--json", action="store_true", help="Output status as JSON (use with -s)"
    )
    parser.add_argument(
        "-p", "--power",
        choices=["quiet", "default", "extreme"],
        help="Set performance mode",
    )
    parser.add_argument(
        "-b", "--battery-limit",
        choices=["on", "off"],
        help="Toggle 80%% battery charge limit",
    )
    parser.add_argument(
        "-c", "--coolboost",
        choices=["on", "off"],
        help="Toggle Acer CoolBoost",
    )
    parser.add_argument(
        "-k", "--kb-timeout",
        choices=["on", "off"],
        help="Toggle keyboard backlight 30s timeout",
    )
    parser.add_argument(
        "-u", "--usb-charge",
        choices=["on", "off"],
        help="Toggle USB charging when powered off",
    )
    parser.add_argument(
        "--cpu-fan",
        choices=["auto", "turbo", "manual"],
        help="Set CPU fan mode",
    )
    parser.add_argument(
        "--cpu-speed",
        type=int, metavar="0-200",
        help="Manual CPU fan speed (0–200, with --cpu-fan manual)",
    )
    parser.add_argument(
        "--gpu-fan",
        choices=["auto", "turbo", "manual"],
        help="Set GPU fan mode",
    )
    parser.add_argument(
        "--gpu-speed",
        type=int, metavar="0-200",
        help="Manual GPU fan speed (0–200, with --gpu-fan manual)",
    )

    args = parser.parse_args()

    if len(sys.argv) == 1:
        parser.print_help()
        sys.exit(0)

    if args.status:
        resp = send_command("GET_STATUS")
        if resp.get("status") == "success":
            if args.json:
                print(json.dumps(resp["data"], indent=4))
            else:
                print_status(resp["data"])
        else:
            print(f"Error: {resp.get('message', 'unknown')}", file=sys.stderr)
            sys.exit(1)

    if args.power:
        resp = send_command(f"SET_POWER_MODE {args.power}")
        print(resp.get("message", ""))

    if args.battery_limit:
        resp = send_command(f"SET_BATTERY_LIMIT {args.battery_limit}")
        print(resp.get("message", ""))

    if args.cpu_fan:
        speed = args.cpu_speed if args.cpu_speed is not None else 100
        speed = max(0, min(200, speed))
        resp = send_command(f"SET_FAN_MODE cpu {args.cpu_fan} {speed}")
        print(resp.get("message", ""))

    if args.gpu_fan:
        speed = args.gpu_speed if args.gpu_speed is not None else 100
        speed = max(0, min(200, speed))
        resp = send_command(f"SET_FAN_MODE gpu {args.gpu_fan} {speed}")
        print(resp.get("message", ""))

    if args.coolboost:
        resp = send_command(f"SET_COOLBOOST {args.coolboost}")
        print(resp.get("message", ""))

    if args.kb_timeout:
        resp = send_command(f"SET_KB_TIMEOUT {args.kb_timeout}")
        print(resp.get("message", ""))

    if args.usb_charge:
        resp = send_command(f"SET_USB_CHARGE {args.usb_charge}")
        print(resp.get("message", ""))


if __name__ == "__main__":
    main()
