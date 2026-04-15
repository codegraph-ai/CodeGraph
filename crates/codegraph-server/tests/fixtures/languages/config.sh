#!/bin/bash

APP_NAME="myapp"
APP_VERSION="1.0.0"
LOG_LEVEL="info"

get_config() {
    local key="$1"
    case "$key" in
        name) echo "$APP_NAME" ;;
        version) echo "$APP_VERSION" ;;
        *) echo "" ;;
    esac
}
