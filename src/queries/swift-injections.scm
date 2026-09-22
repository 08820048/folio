; Parse regex syntax within regex literals. The upstream query also injects a
; nonexistent `comment` language over comments — the highlights query already
; paints those, and no `comment` grammar is registered, so that half is dropped.

((regex_literal) @injection.content
 (#set! injection.language "regex"))
