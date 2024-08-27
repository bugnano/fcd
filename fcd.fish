function fcd
	set tempfile (mktemp)
	command fcd -P $tempfile $argv
	set retval $status

	if test -s $tempfile
		set fcd_pwd (cat $tempfile)
		if test -n $fcd_pwd -a -d $fcd_pwd
			cd -- $fcd_pwd
		end
	end

	command rm -f -- $tempfile

	return $retval
end

