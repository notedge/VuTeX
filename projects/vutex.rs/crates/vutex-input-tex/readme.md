# `vutex-input-tex`

TeX/LaTeX input profile. The syntax frontend stays outside VuTeX and the
document model owns the lowering, so this crate only selects the profile and
reports which constructs reached document semantics.

A construct the profile cannot map is recorded as an explicit conversion
status with a diagnostic code and a source span, never silently dropped or
replaced by an approximation.
