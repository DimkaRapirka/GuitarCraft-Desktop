#!/bin/zsh
# Window id of the main GuitarCraft window (for screencapture -l): the largest layer-0 window of the app process
"$(dirname $0)/winlist" | awk -F'\t' '($3=="guitarcraft" || $4=="GuitarCraft") && $5=="layer=0" { split($6, a, " "); split(a[2], s, "x"); if (s[1] * s[2] > best) { best = s[1] * s[2]; id = $1 } } END { print id }'
