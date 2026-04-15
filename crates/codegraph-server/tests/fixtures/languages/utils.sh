#!/bin/bash
source ./config.sh

log_info() {
    echo "[INFO] $1"
}

log_error() {
    echo "[ERROR] $1" >&2
}

retry() {
    local max_attempts="$1"
    shift
    local count=0
    while [ $count -lt "$max_attempts" ]; do
        if "$@"; then
            return 0
        fi
        count=$((count + 1))
        log_info "Retry $count/$max_attempts"
        sleep 1
    done
    log_error "Failed after $max_attempts attempts"
    return 1
}
