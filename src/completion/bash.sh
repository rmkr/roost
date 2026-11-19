_roost_complete() {
    local current="$2" lead="" stem index candidate
    local words=("${COMP_WORDS[@]:0:COMP_CWORD}")
    stem="${COMP_LINE:0:COMP_POINT}"
    stem="${stem%"$current"}"

    # Readline replaces only the part after a word break. Rejoin the current
    # selector or option value for Roost, then return just the editable part.
    case "${stem##*[[:space:]]}" in
        claude:|codex:|--*=) lead="${stem##*[[:space:]]}" ;;
    esac
    if [[ -n $lead ]]; then
        index=$((${#words[@]} - 1))
        if [[ ${words[index]-} == "${lead: -1}" ]]; then
            unset 'words[index]'
            index=$((index - 1))
        fi
        if [[ ${words[index]-} == "${lead%?}" ]]; then
            unset 'words[index]'
        fi
    fi

    COMPREPLY=()
    while IFS= read -r candidate; do
        COMPREPLY+=("${candidate#"$lead"}")
    done < <(command roost __roost_complete "${words[@]}" "$lead$current")
}
complete -o bashdefault -o default -F _roost_complete roost
