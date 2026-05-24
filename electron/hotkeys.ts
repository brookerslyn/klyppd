import { execSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

let modifiers = 0;
const MOD_ALT = 1;
const MOD_CTRL = 2;
const MOD_SHIFT = 4;
const MOD_SUPER = 8;

const KEY_NAMES: Record<number, string> = {};

// evdev key codes for common keys
const EVDEV_KEYS: Record<number, string> = {
  1: "ESC", 2: "1", 3: "2", 4: "3", 5: "4", 6: "5", 7: "6", 8: "7", 9: "8", 10: "9", 11: "0",
  16: "Q", 17: "W", 18: "E", 19: "R", 20: "T", 21: "Y", 22: "U", 23: "I", 24: "O", 25: "P",
  30: "A", 31: "S", 32: "D", 33: "F", 34: "G", 35: "H", 36: "J", 37: "K", 38: "L",
  44: "Z", 45: "X", 46: "C", 47: "V", 48: "B", 49: "N", 50: "M",
  57: "SPACE",
  59: "F1", 60: "F2", 61: "F3", 62: "F4", 63: "F5", 64: "F6",
  65: "F7", 66: "F8", 67: "F9", 68: "F10", 87: "F11", 88: "F12",
};

// Modifier key codes
const MOD_KEYS: Record<number, number> = {
  56: MOD_ALT,    // KEY_LEFTALT
  100: MOD_ALT,   // KEY_RIGHTALT
  29: MOD_CTRL,   // KEY_LEFTCTRL
  97: MOD_CTRL,   // KEY_RIGHTCTRL
  42: MOD_SHIFT,  // KEY_LEFTSHIFT
  54: MOD_SHIFT,  // KEY_RIGHTSHIFT
  125: MOD_SUPER, // KEY_LEFTMETA
  126: MOD_SUPER, // KEY_RIGHTMETA
};

function isKeyboard(devicePath: string): boolean {
  try {
    const fd = fs.openSync(devicePath, "r");
    fs.closeSync(fd);
    // Check if the device has the KEY_A capability via sysfs
    const basename = path.basename(devicePath);
    const capsPath = `/sys/class/input/${basename}/device/capabilities/key`;
    if (fs.existsSync(capsPath)) {
      const caps = fs.readFileSync(capsPath, "utf-8").trim();
      // KEY_A is code 30, which falls in the first capability word
      return caps.length > 0;
    }
    return true;
  } catch {
    return false;
  }
}

function hotkeyMatches(hotkeyStr: string, keyCode: number): boolean {
  const parts = hotkeyStr.split("+").map((s) => s.trim());
  if (parts.length === 0) return false;

  const mainKey = parts[parts.length - 1].toUpperCase();
  const pressedName = EVDEV_KEYS[keyCode];
  if (!pressedName || pressedName !== mainKey) return false;

  const needAlt = parts.some((p) => p.toLowerCase() === "alt");
  const needCtrl = parts.some((p) => p.toLowerCase() === "ctrl");
  const needShift = parts.some((p) => p.toLowerCase() === "shift");
  const needSuper = parts.some(
    (p) => p.toLowerCase() === "super" || p.toLowerCase() === "meta"
  );

  return (
    needAlt === !!(modifiers & MOD_ALT) &&
    needCtrl === !!(modifiers & MOD_CTRL) &&
    needShift === !!(modifiers & MOD_SHIFT) &&
    needSuper === !!(modifiers & MOD_SUPER)
  );
}

export function startEvdevHotkeys(
  getSettings: () => {
    hotkey_save_replay: string;
    hotkey_start_stop_recording: string;
    hotkey_start_stop_buffer: string;
  },
  onHotkey: (cmd: string) => void
): void {
  // Find keyboard devices
  const inputDir = "/dev/input";
  if (!fs.existsSync(inputDir)) {
    console.error("klyppd: /dev/input not found");
    return;
  }

  let lastClip = 0;

  function pollDevices(): void {
    try {
      const entries = fs.readdirSync(inputDir).filter((e) => e.startsWith("event"));
      
      for (const entry of entries) {
        const devicePath = path.join(inputDir, entry);
        try {
          const fd = fs.openSync(devicePath, fs.constants.O_RDONLY | fs.constants.O_NONBLOCK);
          const buf = Buffer.alloc(24); // struct input_event on 64-bit
          
          let bytesRead: number;
          try {
            bytesRead = fs.readSync(fd, buf, 0, 24, null);
          } catch (e: any) {
            if (e.code === "EAGAIN") {
              fs.closeSync(fd);
              continue;
            }
            fs.closeSync(fd);
            continue;
          }

          while (bytesRead === 24) {
            // Parse input_event: time(16 bytes), type(2), code(2), value(4)
            const type = buf.readUInt16LE(16);
            const code = buf.readUInt16LE(18);
            const value = buf.readInt32LE(20);

            if (type === 1) {
              // EV_KEY
              const modBit = MOD_KEYS[code];
              if (modBit !== undefined) {
                if (value !== 0) {
                  modifiers |= modBit;
                } else {
                  modifiers &= ~modBit;
                }
              }

              if (value === 1) {
                // key down
                const settings = getSettings();
                const now = Date.now();

                if (hotkeyMatches(settings.hotkey_save_replay, code)) {
                  if (now - lastClip > 1500) {
                    lastClip = now;
                    onHotkey("save-replay");
                  }
                } else if (
                  hotkeyMatches(settings.hotkey_start_stop_recording, code)
                ) {
                  onHotkey("toggle-recording");
                } else if (
                  hotkeyMatches(settings.hotkey_start_stop_buffer, code)
                ) {
                  onHotkey("toggle-buffer");
                }
              }
            }

            try {
              bytesRead = fs.readSync(fd, buf, 0, 24, null);
            } catch {
              break;
            }
          }

          fs.closeSync(fd);
        } catch {}
      }
    } catch {}
  }

  setInterval(pollDevices, 5);
}
