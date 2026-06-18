// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

/**
 * Machine-profile detection for `engagement.machineProfile`.
 *
 * Purpose: fingerprint the graph_load crash cohort — which classes of machine
 * corrupt their graph.db (cloud-synced data dir, third-party AV holding file
 * handles, VM/virtual-disk fsync semantics, low RAM). Each detector MAY read a
 * path, a MAC OUI, or an AV product name to CLASSIFY, but only ever returns a
 * fixed enum / coarse number. No path, MAC, hostname, product name, or exact
 * byte count ever leaves this module — the reporter buckets/allowlists the
 * output again before it is sent. Best-effort; never throws.
 */

import * as os from 'os';
import * as fs from 'fs';
import * as path from 'path';
import { execFile } from 'child_process';

export interface MachineProfile {
    /** local | cloud_onedrive | cloud_other | unc_network | unknown */
    dataDirKind: string;
    /** physical | vm | wsl | container | unknown */
    machineKind: string;
    /** total physical RAM in GB (reporter buckets it) */
    totalRamGb: number;
    /** defender_only | third_party | none | unknown (Windows only) */
    antivirusKind: string;
}

/**
 * Classify where `~/.codegraph` lives WITHOUT logging the path. Cloud-sync
 * agents (OneDrive Known Folder Move, Dropbox, …) and network/UNC profiles
 * hold handles + rewrite files under RocksDB → torn-write corruption.
 */
function detectDataDirKind(): string {
    try {
        const dir = path.join(os.homedir(), '.codegraph');
        if (dir.startsWith('\\\\')) return 'unc_network';
        const lower = dir.toLowerCase();
        const oneDriveEnv = [
            process.env.OneDrive,
            process.env.OneDriveConsumer,
            process.env.OneDriveCommercial,
        ]
            .filter((v): v is string => !!v)
            .map((v) => v.toLowerCase());
        if (oneDriveEnv.some((p) => lower.startsWith(p)) || lower.includes('onedrive')) {
            return 'cloud_onedrive';
        }
        if (/dropbox|google[ _]?drive|\bbox\b|pcloud|icloud|nextcloud|\bmega\b/.test(lower)) {
            return 'cloud_other';
        }
        return 'local';
    } catch {
        return 'unknown';
    }
}

const VM_MAC_OUI_PREFIXES = [
    '00:05:69', '00:0c:29', '00:1c:14', '00:50:56', // VMware
    '08:00:27', '0a:00:27', // VirtualBox
    '00:15:5d', // Hyper-V
    '52:54:00', // QEMU/KVM
    '00:16:3e', // Xen
];

/** Physical vs virtualized. MAC is read only to test its OUI prefix; the MAC
 * itself is never emitted. */
function detectMachineKind(): string {
    try {
        if (process.platform === 'linux') {
            if (process.env.WSL_DISTRO_NAME) return 'wsl';
            try {
                const rel = os.release().toLowerCase();
                if (rel.includes('microsoft') || rel.includes('wsl')) return 'wsl';
            } catch {
                /* ignore */
            }
            try {
                if (fs.existsSync('/.dockerenv')) return 'container';
                const cg = fs.readFileSync('/proc/1/cgroup', 'utf8');
                if (/docker|containerd|kubepods|\blxc\b/.test(cg)) return 'container';
            } catch {
                /* ignore */
            }
        }
        const ifaces = os.networkInterfaces();
        for (const name of Object.keys(ifaces)) {
            for (const ni of ifaces[name] ?? []) {
                const mac = (ni.mac || '').toLowerCase();
                if (mac && mac !== '00:00:00:00:00:00' && VM_MAC_OUI_PREFIXES.some((p) => mac.startsWith(p))) {
                    return 'vm';
                }
            }
        }
        return 'physical';
    } catch {
        return 'unknown';
    }
}

/** Windows AV class via SecurityCenter2. Returns only the coarse class — never
 * the product name. Off the activation path (async, hard-timeout). */
function detectAntivirusKind(): Promise<string> {
    if (process.platform !== 'win32') return Promise.resolve('unknown');
    return new Promise((resolve) => {
        const ps =
            'Get-CimInstance -Namespace root/SecurityCenter2 -ClassName AntiVirusProduct ' +
            '| Select-Object -ExpandProperty displayName';
        try {
            execFile(
                'powershell.exe',
                ['-NoProfile', '-NonInteractive', '-Command', ps],
                { timeout: 4000, windowsHide: true },
                (err, stdout) => {
                    if (err || !stdout) {
                        resolve('unknown');
                        return;
                    }
                    const names = stdout
                        .split(/\r?\n/)
                        .map((s) => s.trim().toLowerCase())
                        .filter(Boolean);
                    if (names.length === 0) {
                        resolve('none');
                        return;
                    }
                    const isDefender = (n: string) =>
                        n.includes('defender') || n.includes('microsoft security');
                    resolve(names.some((n) => !isDefender(n)) ? 'third_party' : 'defender_only');
                },
            );
        } catch {
            resolve('unknown');
        }
    });
}

export async function detectMachineProfile(): Promise<MachineProfile> {
    return {
        dataDirKind: detectDataDirKind(),
        machineKind: detectMachineKind(),
        totalRamGb: os.totalmem() / 1024 ** 3,
        antivirusKind: await detectAntivirusKind(),
    };
}
