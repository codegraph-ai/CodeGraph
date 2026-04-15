#!/bin/bash
source ./lib.sh

greet() {
    local name="$1"
    echo "Hello, $name"
}

function deploy() {
    if [ -z "$1" ]; then
        echo "Usage: deploy <env>"
        return 1
    fi
    greet "$1"
}

deploy "$@"
