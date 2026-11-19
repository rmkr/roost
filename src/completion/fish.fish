function __roost_complete
    set -l current (commandline --current-token --tokenize --cut-at-cursor)
    set -l candidates (command roost __roost_complete (commandline --current-process --tokenize --cut-at-cursor) "$current")
    if set -q candidates[1]
        printf '%s\n' $candidates
    else
        set -l raw (commandline --current-token --cut-at-cursor)
        __fish_complete_path "$raw"
    end
end
complete --command roost --no-files --arguments '(__roost_complete)'
