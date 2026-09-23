#!/bin/zsh
# ama — jump to the folder of a document open in Amalith.
#
# Installed from Amalith ▸ Preferences ▸ Integrations ▸ CLI. Changing the
# calling shell's directory is something only that shell can do, so this is a
# function sourced into your shell rather than a binary on PATH.

ama() {
  setopt localoptions localtraps no_shwordsplit

  if [[ "$1" == "-h" || "$1" == "--help" ]]; then
    print "Usage: ama"
    print "Pick a document open in Amalith and cd into the folder beside its file."
    return 0
  fi

  # Where the app publishes what it has open. Mirrors `settings::config_dir`.
  local store
  if [[ "$OSTYPE" == darwin* ]]; then
    store="$HOME/Library/Application Support/Amalith/open-documents.tsv"
  elif [[ -n "${XDG_CONFIG_HOME:-}" ]]; then
    store="$XDG_CONFIG_HOME/amalith/open-documents.tsv"
  else
    store="$HOME/.config/amalith/open-documents.tsv"
  fi

  if [[ ! -r "$store" ]]; then
    print -u2 "ama: Amalith isn't running"
    return 1
  fi

  local -a lines
  lines=("${(@f)$(< $store)}")

  # First line is the app's process id. The file is only removed on a clean
  # quit, so without this check a crash would leave ama offering documents
  # from a session that's gone.
  local pid="${lines[1]}"
  if [[ -z "$pid" ]] || ! kill -0 "$pid" 2>/dev/null; then
    print -u2 "ama: Amalith isn't running"
    return 1
  fi

  # Remaining rows are: dirty flag, title, path. The path is empty for a
  # document that has never been saved, the one case ENTER can't act on.
  local -a titles paths flags
  local row rest
  for row in "${lines[@]:1}"; do
    [[ -z "$row" ]] && continue
    flags+=("${row%%$'\t'*}")
    rest="${row#*$'\t'}"
    titles+=("${rest%%$'\t'*}")
    paths+=("${rest#*$'\t'}")
  done

  local -i count=${#titles[@]}
  if (( count == 0 )); then
    print -u2 "ama: no documents open in Amalith"
    return 1
  fi

  if [[ ! -t 0 || ! -t 1 ]]; then
    print -u2 "ama: picker needs a tty"
    return 1
  fi

  local -a labels
  local -i i
  for (( i = 1; i <= count; i++ )); do
    if [[ "${flags[$i]}" == 1 ]]; then
      labels+=("${titles[$i]}*")
    else
      labels+=("${titles[$i]}")
    fi
  done

  local -i drawn=0
  local -i menu_lines=0

  local query=""
  local -a filtered
  local -i selected=1

  _ama_filter() {
    filtered=()
    local -i idx
    if [[ -z "$query" ]]; then
      for (( idx = 1; idx <= count; idx++ )); do
        filtered+=("$idx")
      done
    else
      local q_lower="${query:l}"
      for (( idx = 1; idx <= count; idx++ )); do
        [[ "${labels[$idx]:l}" == *"$q_lower"* ]] && filtered+=("$idx")
      done
    fi
  }

  _ama_filter

  local ama_stty_orig
  ama_stty_orig="$(stty -g 2>/dev/null)"
  stty -echo 2>/dev/null

  _ama_cleanup() {
    printf '\e[?25h'
    [[ -n "$ama_stty_orig" ]] && stty "$ama_stty_orig" 2>/dev/null
    unfunction _ama_cleanup _ama_erase_menu _ama_draw_menu _ama_filter 2>/dev/null
  }
  _ama_erase_menu() {
    if (( drawn )); then
      printf '\e[%dF\e[J' "$menu_lines"
      drawn=0
    fi
  }
  _ama_draw_menu() {
    if (( drawn )); then
      printf '\e[%dF' "$menu_lines"
    fi
    printf '\e[J'

    local -i lines=0
    local -i cols=${COLUMNS:-80}

    local c_reset=$'\e[0m'
    local c_dim=$'\e[2m'
    local c_header=$'\e[38;2;188;147;249m'
    local c_sel_bg=$'\e[48;2;55;60;82m'
    local c_text=$'\e[1m\e[38;2;235;235;245m'
    local c_muted=$'\e[38;5;245m'
    local c_dot=$'\e[38;2;78;186;101m'
    local c_warn=$'\e[38;2;229;181;103m'

    local title="amalith"
    local -i n=${#filtered[@]}

    # The header previews where ENTER would land, so arrowing through the
    # list shows the destination before committing to it.
    local display header_color=$c_dim
    if (( n > 0 )); then
      local sel_path="${paths[${filtered[selected]}]}"
      if [[ -n "$sel_path" ]]; then
        display="${${sel_path:h}/#$HOME/~}"
      else
        display="not saved yet"
        header_color=$c_warn
      fi
    else
      display=""
    fi

    local -i box_w=$(( ${#title} + ${#display} + 3 ))
    local -i fi idx llen
    for (( fi = 1; fi <= n; fi++ )); do
      idx=${filtered[fi]}
      llen=$(( ${#labels[$idx]} + 6 ))
      (( llen > box_w )) && box_w=$llen
    done
    local -i qlen=$(( ${#query} + 10 ))
    (( qlen > box_w )) && box_w=$qlen
    (( box_w < 30 )) && box_w=30
    local -i max_w=$(( cols - 4 ))
    (( box_w > max_w )) && box_w=$max_w

    local rule=""
    local -i d
    for (( d = 0; d < box_w; d++ )); do rule+="─"; done

    print -r -- "  ${c_header}${title}${c_reset}  ${header_color}${display}${c_reset}"; (( lines++ ))
    print -r -- "  ${c_dim}${rule}${c_reset}"; (( lines++ ))

    if [[ -n "$query" ]]; then
      print -r -- "  ${c_header}⌕${c_reset}  ${query}${c_dim}▏${c_reset}"
    else
      print -r -- "  ${c_header}⌕${c_reset}  ${c_dim}Type to search…${c_reset}"
    fi
    (( lines++ ))

    print -r -- "  ${c_dim}${rule}${c_reset}"; (( lines++ ))

    if (( n == 0 )); then
      print -r -- "  ${c_dim}No matches${c_reset}"; (( lines++ ))
    else
      local label_text spaces symbol
      local -i pad p
      for (( fi = 1; fi <= n; fi++ )); do
        idx=${filtered[fi]}
        label_text="${labels[$idx]}"
        # A hollow dot for a saved document, a dash for one with no file
        # behind it yet — the rows ENTER can't act on.
        symbol="○"
        [[ -z "${paths[$idx]}" ]] && symbol="–"

        if (( fi == selected )); then
          pad=$(( box_w - ${#label_text} - 5 ))
          (( pad < 0 )) && pad=0
          spaces=""
          for (( p = 0; p < pad; p++ )); do spaces+=" "; done
          print -r -- "  ${c_sel_bg} ${c_dot}${symbol}${c_text}  ${label_text}${spaces} ${c_reset}"
        else
          print -r -- "   ${c_dot}${symbol}${c_reset}  ${c_muted}${label_text}${c_reset}"
        fi
        (( lines++ ))
      done
    fi

    print -r -- "  ${c_dim}${rule}${c_reset}"; (( lines++ ))
    print -r -- "  ${c_dim}↑↓ select   ENTER choose   ESC cancel${c_reset}"; (( lines++ ))

    menu_lines=$lines
    drawn=1
  }

  trap _ama_cleanup EXIT
  trap '_ama_erase_menu; _ama_cleanup; trap - EXIT; return 130' INT TERM

  printf '\e[?25l'
  _ama_draw_menu

  local key rest_key
  while true; do
    key=''
    if ! IFS= read -rk1 key; then
      _ama_erase_menu
      return 1
    fi

    case "$key" in
      $'\e')
        rest_key=''
        IFS= read -rk2 -t 0.05 rest_key 2>/dev/null
        case "$rest_key" in
          '[A'|'OA')
            local -i n=${#filtered[@]}
            if (( n > 0 )); then
              (( selected-- ))
              (( selected < 1 )) && selected=$n
            fi
            ;;
          '[B'|'OB')
            local -i n=${#filtered[@]}
            if (( n > 0 )); then
              (( selected++ ))
              (( selected > n )) && selected=1
            fi
            ;;
          '')
            _ama_erase_menu
            return 0
            ;;
          *)
            # unrecognized escape sequence (e.g. left/right arrow) — ignore
            ;;
        esac
        _ama_draw_menu
        ;;
      $'\x7f'|$'\b')
        if (( ${#query} > 0 )); then
          query="${query[1,-2]}"
          _ama_filter
          selected=1
          _ama_draw_menu
        fi
        ;;
      $'\n'|$'\r')
        local -i n=${#filtered[@]}
        (( n == 0 )) && continue
        local -i sel_idx=${filtered[selected]}

        _ama_erase_menu
        _ama_cleanup
        trap - EXIT INT TERM

        local doc_path="${paths[$sel_idx]}"
        if [[ -z "$doc_path" ]]; then
          print -u2 "ama: \"${titles[$sel_idx]}\" hasn't been saved, so there's no folder to open."
          print -u2 "ama: save it in Amalith (⌘S) and run ama again."
          return 1
        fi

        local dir="${doc_path:h}"
        if [[ ! -d "$dir" ]]; then
          print -u2 "ama: that folder is gone: $dir"
          return 1
        fi

        builtin cd "$dir" || return 1
        return 0
        ;;
      *)
        if [[ -n "$key" ]]; then
          query+="$key"
          _ama_filter
          selected=1
          _ama_draw_menu
        fi
        ;;
    esac
  done
}
