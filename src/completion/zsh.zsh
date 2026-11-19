_roost_complete() {
    local -a candidates
    candidates=("${(@f)$(command roost __roost_complete "${words[@]:0:$((CURRENT - 1))}" "$PREFIX")}")
    compadd -a candidates || _default
}
compdef _roost_complete roost
