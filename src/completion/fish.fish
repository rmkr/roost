function __roost_complete
    set -l current (commandline --current-token --cut-at-cursor)
    set -l candidates (command roost __roost_complete (commandline --current-process --tokenize --cut-at-cursor) "$current")
    if set -q candidates[1]
        printf '%s\n' $candidates
    else
        __fish_complete_path "$current"
    end
end
complete --command roost --no-files --arguments '(__roost_complete)'
