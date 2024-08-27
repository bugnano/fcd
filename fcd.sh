fcd() {
	local tempfile=$(mktemp)
	command fcd -P "$tempfile" "$@"
	local retval=$?

	if test -s "$tempfile"; then
		local fcd_pwd=$(cat $tempfile)
		if test -n "$fcd_pwd" -a -d "$fcd_pwd"; then
			 cd -- "$fcd_pwd"
		fi
	fi

	command rm -f -- "$tempfile"

	return $retval
}

