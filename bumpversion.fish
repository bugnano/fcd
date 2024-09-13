#! /usr/bin/env fish

set new_version 1.0.0
set new_date (date '+%Y-%m-%d')
set new_year (date '+%Y')

sed -i -e "s/^\(\s*version\s*=\s*\).*/\1\"$new_version\"/g" Cargo.toml

for f in doc/fcd.1.adoc doc/fcd-view.1.adoc
	sed -i \
		-e "s/^\(\s*:man version:\s*\).*/\1$new_version/g" \
		-e "s/^\(\s*:revdate:\s*\).*/\1$new_date/g" \
		$f
end

for f in (rg -l '[(]C[)] 2023-[0-9]+')
	sed -i -e "s/[(]C[)] 2023-[0-9]\+/(C) 2023-$new_year/g" $f
end

