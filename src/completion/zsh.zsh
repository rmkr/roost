_roost_complete() {
    local output
    output=$(command roost __roost_complete "${words[@]:0:$((CURRENT - 1))}" "$PREFIX")
    if [[ -z $output ]]; then
        _default
        return
    fi
    local -a candidates
    candidates=("${(@f)output}")
    compadd -a candidates || _default
}
compdef _roost_complete roost
