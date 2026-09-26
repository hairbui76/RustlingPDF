; RustlingPDF NSIS installer hooks (bundle.windows.nsis.installerHooks).
;
; Registers the same Windows Explorer context menu the MSI authors in
; ../wix/provisioning.wxs, so the `*-setup.exe` download gets it too:
;
;   - the "RustlingPDF" cascade on .pdf files (Open / Merge / Compress /
;     Convert), and
;   - the flat "Convert to PDF with RustlingPDF" verb on convertible non-PDF
;     files.
;
; Key paths, labels and `--tool <action>` commands are identical to the MSI's,
; so a machine that has run both installers ends up with one menu, not two.
; rust/contracts/desktop-explorer-context-menu.md is the contract;
; convertToPdfMenu.test.ts pins this file to provisioning.wxs.
;
; Kept out of the forked installer.nsi on purpose: the template stays
; byte-identical to upstream apart from its marked fork notes, and it already
; includes this file through `{{installer_hooks}}`. Both hooks run after
; SetContext, so SHCTX is HKLM for this perMachine installer.
;
; ${MAINBINARYNAME} is only referenced inside macro bodies: this file is
; included before installer.nsi defines it, and macro bodies are expanded where
; they are inserted (the Install/Uninstall sections), not where they are read.
; HKLM\SOFTWARE\Classes\SystemFileAssociations is shared between the 32- and
; 64-bit registry views, so no SetRegView is needed.

!define RUSTLING_SFA "Software\Classes\SystemFileAssociations"
!define RUSTLING_PDF_CASCADE "${RUSTLING_SFA}\.pdf\shell\RustlingPDF"
!define RUSTLING_CONVERT_VERB "RustlingPDF.ConvertToPdf"

; The extensions that get the Convert to PDF verb. MUST equal the
; ConvertToPdfExtensions define in provisioning.wxs, in the same order
; (enforced by convertToPdfMenu.test.ts).
!macro RUSTLING_FOR_EACH_CONVERT_TO_PDF_EXTENSION _MACRO
  !insertmacro ${_MACRO} "doc"
  !insertmacro ${_MACRO} "docx"
  !insertmacro ${_MACRO} "odt"
  !insertmacro ${_MACRO} "rtf"
  !insertmacro ${_MACRO} "xls"
  !insertmacro ${_MACRO} "xlsx"
  !insertmacro ${_MACRO} "ods"
  !insertmacro ${_MACRO} "ppt"
  !insertmacro ${_MACRO} "pptx"
  !insertmacro ${_MACRO} "odp"
  !insertmacro ${_MACRO} "jpg"
  !insertmacro ${_MACRO} "jpeg"
  !insertmacro ${_MACRO} "png"
  !insertmacro ${_MACRO} "gif"
  !insertmacro ${_MACRO} "bmp"
  !insertmacro ${_MACRO} "tiff"
  !insertmacro ${_MACRO} "webp"
  !insertmacro ${_MACRO} "svg"
!macroend

!macro RUSTLING_WRITE_PDF_CASCADE_VERB _KEY _LABEL _ACTION
  WriteRegStr SHCTX "${RUSTLING_PDF_CASCADE}\shell\${_KEY}" "MUIVerb" "${_LABEL}"
  WriteRegStr SHCTX "${RUSTLING_PDF_CASCADE}\shell\${_KEY}" "MultiSelectModel" "Player"
  WriteRegStr SHCTX "${RUSTLING_PDF_CASCADE}\shell\${_KEY}\command" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\" --tool ${_ACTION} $\"%1$\""
!macroend

!macro RUSTLING_WRITE_CONVERT_TO_PDF_VERB _EXT
  WriteRegStr SHCTX "${RUSTLING_SFA}\.${_EXT}\shell\${RUSTLING_CONVERT_VERB}" "MUIVerb" "Convert to PDF with RustlingPDF"
  WriteRegStr SHCTX "${RUSTLING_SFA}\.${_EXT}\shell\${RUSTLING_CONVERT_VERB}" "Icon" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\",0"
  WriteRegStr SHCTX "${RUSTLING_SFA}\.${_EXT}\shell\${RUSTLING_CONVERT_VERB}" "MultiSelectModel" "Player"
  WriteRegStr SHCTX "${RUSTLING_SFA}\.${_EXT}\shell\${RUSTLING_CONVERT_VERB}\command" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\" --tool convert $\"%1$\""
!macroend

; Uninstall removes a verb only while its command still launches THIS
; installation — the same guard the template applies to deep links. If the
; MSI (or a second copy in another directory) has since written the same key,
; that registration is left alone. Afterwards the `shell` and `.<ext>` parents
; are removed only if they are now empty (/ifempty), so another application's
; verbs under the same extension are never touched.
!macro RUSTLING_DELETE_CONVERT_TO_PDF_VERB _EXT
  ReadRegStr $R7 SHCTX "${RUSTLING_SFA}\.${_EXT}\shell\${RUSTLING_CONVERT_VERB}\command" ""
  ${If} $R7 == "$\"$INSTDIR\${MAINBINARYNAME}.exe$\" --tool convert $\"%1$\""
    DeleteRegKey SHCTX "${RUSTLING_SFA}\.${_EXT}\shell\${RUSTLING_CONVERT_VERB}"
    DeleteRegKey /ifempty SHCTX "${RUSTLING_SFA}\.${_EXT}\shell"
    DeleteRegKey /ifempty SHCTX "${RUSTLING_SFA}\.${_EXT}"
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; .pdf cascade: MUIVerb + an EMPTY SubCommands value makes Explorer
  ; enumerate the `shell` subkey; the NN_ prefixes order the submenu.
  WriteRegStr SHCTX "${RUSTLING_PDF_CASCADE}" "MUIVerb" "RustlingPDF"
  WriteRegStr SHCTX "${RUSTLING_PDF_CASCADE}" "SubCommands" ""
  WriteRegStr SHCTX "${RUSTLING_PDF_CASCADE}" "Icon" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\",0"
  !insertmacro RUSTLING_WRITE_PDF_CASCADE_VERB "01_open" "Open" "open"
  !insertmacro RUSTLING_WRITE_PDF_CASCADE_VERB "02_merge" "Merge" "merge"
  !insertmacro RUSTLING_WRITE_PDF_CASCADE_VERB "03_compress" "Compress" "compress"
  !insertmacro RUSTLING_WRITE_PDF_CASCADE_VERB "04_convert" "Convert" "convert"

  !insertmacro RUSTLING_FOR_EACH_CONVERT_TO_PDF_EXTENSION RUSTLING_WRITE_CONVERT_TO_PDF_VERB
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ReadRegStr $R7 SHCTX "${RUSTLING_PDF_CASCADE}\shell\01_open\command" ""
  ${If} $R7 == "$\"$INSTDIR\${MAINBINARYNAME}.exe$\" --tool open $\"%1$\""
    DeleteRegKey SHCTX "${RUSTLING_PDF_CASCADE}"
    DeleteRegKey /ifempty SHCTX "${RUSTLING_SFA}\.pdf\shell"
    DeleteRegKey /ifempty SHCTX "${RUSTLING_SFA}\.pdf"
  ${EndIf}

  !insertmacro RUSTLING_FOR_EACH_CONVERT_TO_PDF_EXTENSION RUSTLING_DELETE_CONVERT_TO_PDF_VERB
!macroend
