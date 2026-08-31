# Turns a sorted list of commit timestamps into the card drawn at the top of the
# README, and into the markdown block that points at it.
#
# Driven by coding-time, which supplies every variable below with -v. Reads
# one Unix timestamp per line, oldest first.
#
#   gap         seconds that end a sitting
#   opening     minutes credited to the commit that opens one
#   recent      how many days back still counts as "recent"
#   days        how many days of chart to draw at most
#   now         seconds since the epoch, taken once by the caller
#   card        file to write the SVG to
#   block       file to write the markdown to

# Every date here is a UTC calendar day, which a Unix timestamp divided by 86400
# already is: UTC has no leap seconds to throw the division off.
BEGIN {
	today = int(now / 86400)
	split("Jan Feb Mar Apr May Jun Jul Aug Sep Oct Nov Dec", MONTH, " ")
}

# Turns one of those days back into the date it names. Done here rather than by
# the caller because only awk knows how far back the chart reached, and done by
# hand because the awk macOS ships has no strftime.
function civil(epoch_day,   z, era, doe, yoe, doy, mp, year, month, day) {
	z = epoch_day + 719468
	era = int(z / 146097)
	doe = z - era * 146097
	yoe = int((doe - int(doe / 1460) + int(doe / 36524) - int(doe / 146096)) / 365)
	doy = doe - (365 * yoe + int(yoe / 4) - int(yoe / 100))
	mp = int((5 * doy + 2) / 153)
	year = yoe + era * 400
	month = mp + (mp < 10 ? 3 : -9)
	day = doy - int((153 * mp + 2) / 5) + 1
	# March is where the algorithm starts its year, so January and February belong
	# to the one after it.
	if (month <= 2) {
		year++
	}
	return day " " MONTH[month] " " year
}

# The same date without its year, which is how a last-played date is usually
# written and how this card writes one: the axis below already names the years.
function short_civil(epoch_day,   parts) {
	split(civil(epoch_day), parts, " ")
	return parts[1] " " parts[2]
}

# Groups the whole part of a number in threes, so a count that has grown long
# stays readable. The fraction is left alone: it never runs past one digit here.
function grouped(number,   point, whole, rest, out) {
	point = index(number, ".")
	whole = (point > 0) ? substr(number, 1, point - 1) : number
	rest = (point > 0) ? substr(number, point) : ""
	while (length(whole) > 3) {
		out = "," substr(whole, length(whole) - 2) out
		whole = substr(whole, 1, length(whole) - 3)
	}
	return whole out rest
}

# A tenth of an hour is worth showing; a zero one is just a decimal point with
# nothing behind it.
function hours(minutes,   figure) {
	figure = sprintf("%.1f", minutes / 60)
	sub(/\.0$/, "", figure)
	return figure
}

# A count of one takes a singular noun. The test is on the count rather than on
# what grouped() returned, which may carry a thousands separator, and on the raw
# figure rather than a rounded one: "1.0" is stripped to "1" by hours() before it
# ever reaches here.
function plural(count, word) {
	return (count == 1) ? word : word "s"
}

# Roughly how wide a string runs at a given font size in the card's font stack.
# SVG cannot measure text, so the card is sized from an estimate rather than
# from the glyphs themselves.
function width_of(text, size) {
	return length(text) * size * 0.52
}

function svg(line) {
	print line > card
}

function markdown(line) {
	print line > block
}

# Credits the time a moment earned and files it under the day it fell on. Called
# once per commit, and once more for the present, which no commit speaks for yet.
function credit(moment, opened, minutes, day) {
	# Whether a moment opened a sitting is a fact about the gap, not about the
	# minutes it earned: a real gap of exactly `opening` minutes would otherwise
	# be mistaken for one.
	opened = (moments == 0 || moment - previous > gap)
	minutes = opened ? opening : (moment - previous) / 60
	if (opened) {
		sittings++
	}
	total += minutes

	# Calendar days counted back from today, so a bar holds exactly what `git log
	# --date=short` files under that date rather than a 24 hours that end whenever
	# this ran. A commit dated in the future — a skewed clock, a hand-set date —
	# would count backwards past today, so it is pinned to today instead.
	day = today - int(moment / 86400)
	if (day < 0) {
		day = 0
	}

	# Counted off the same days as the bars, so "the last 14 days" means fourteen
	# dates ending today rather than fourteen times 24 hours ending at whatever
	# o'clock this happened to run.
	if (day < recent) {
		recently += minutes
	}

	if (day < days) {
		by_day[day] += minutes
		if (by_day[day] > tallest) {
			tallest = by_day[day]
		}
	}
	if (day + 1 > lived) {
		lived = day + 1
	}

	previous = moment
	moments++
}

{
	# The stream arrives sorted, so the first line is the oldest commit there is.
	# Read here rather than from `git log`, whose order is commit date and parts
	# company with author date the moment anything is rebased.
	if (commits == 0) {
		oldest = $1
	}
	credit($1)
	commits++
}

END {
	if (commits == 0) {
		exit
	}

	# The commit being made is not in the history this reads: a pre-commit hook
	# runs before that commit exists. Crediting the present closes the gap, so the
	# card describes the commit it is about to travel in rather than the one
	# before it — counted as well as timed, since a card that ships inside a commit
	# it does not count is a card one behind the log. Nothing accumulates: every
	# run recomputes from the log and adds exactly one present.
	if (now > previous) {
		credit(now)
		commits++
	}

	# One bar per day the repository has existed, up to the window asked for, so
	# a young project is not padded out with days that predate it.
	if (lived < days) {
		days = lived
	}
	if (days < 1) {
		days = 1
	}

	# The whole history, however little of it the chart has room to draw: the
	# headline counts every commit, so the axis names every day. The right end is
	# today rather than the newest commit for the same reason the present is
	# credited — the commit being made is one the card speaks for.
	first_seen = civil(int(oldest / 86400))
	last_seen = civil(today)
	# The day the axis ends on: the card ships inside the commit being made, so the
	# last day worked is today rather than the newest commit already in the log.
	last_played = short_civil(today)

	headline = hours(total)
	played = grouped(headline) " " plural(headline, "hour")
	recent_hours = hours(recently)
	summary = grouped(recent_hours) " " plural(recent_hours, "hour") \
			" in the last " recent " " plural(recent, "day") "   \302\267   " \
			grouped(sittings) " " plural(sittings, "sitting") "   \302\267   " \
			grouped(commits) " " plural(commits, "commit")

	PAD = 24
	LABEL_Y = 40
	HEADLINE_Y = 62
	BASE = 134
	AXIS_Y = 152
	CEILING = 46
	STAT_SIZE = 16
	SUMMARY_SIZE = 12
	HEIGHT = 168
	MINIMUM_BAR = 3

	# Width follows the text, and the chart is then stretched across whatever that
	# leaves, so the card reads the same at any number of days. The date sits above
	# the summary and is much the shorter of the two, so the summary still sets this.
	width = int(PAD + width_of(played, STAT_SIZE) + 46 + width_of(summary, SUMMARY_SIZE) + PAD)
	span = width - PAD * 2
	# How wide the gap between two neighbouring bars is. It follows how much room a
	# day has, so a dense chart keeps its spacing, but it is capped: a gap is judged
	# in pixels rather than in proportion, and a third of a day's room is a hair at
	# fifty days and half the card at one.
	#
	# Named for the bars because `gap` is already the pause that ends a sitting.
	bar_gap = span * 0.32 / days
	if (bar_gap > 6) {
		bar_gap = 6
	}
	# A gap sits between two bars, so there are days - 1 of them rather than one per
	# bar. Take them out of the span and the bars share what is left, which is what
	# lets a single day fill the chart: one bar, and no gap to leave room for.
	bar = int((span - (days - 1) * bar_gap) / days)
	if (bar < 2) {
		bar = 2
	}
	# The step from one bar to the next, measured from the width the bars actually
	# rounded to, so the last one lands on the margin rather than short of it by the
	# truncation collected along the way.
	bar_step = (days > 1) ? (span - bar) / (days - 1) : 0

	printf "" > card
	svg("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"" width "\" height=\"" HEIGHT "\"" \
			" viewBox=\"0 0 " width " " HEIGHT "\" role=\"img\"" \
			" aria-label=\"Play time: " headline " " plural(headline, "hour") \
					" over " commits " " plural(commits, "commit") "," \
					" last played " last_played "\">")
	svg("  <style>")
	svg("    .card { fill: #0d1117; stroke: #30363d }")
	svg("    text { font-family: -apple-system, BlinkMacSystemFont, \"Segoe UI\", Helvetica, Arial, sans-serif }")
	svg("    .label { font-size: 11px; letter-spacing: 1.7px; fill: #8b949e }")
	svg("    .stat { font-size: " STAT_SIZE "px; font-weight: 600; letter-spacing: normal; fill: #e6edf3 }")
	svg("    .note { font-size: " SUMMARY_SIZE "px; fill: #8b949e }")
	svg("    .axis { font-size: 10px; fill: #6e7681 }")
	svg("    .bar { fill: #388bfd }")
	svg("    .bar-quiet { fill: #21262d }")
	svg("  </style>")
	svg("  <rect class=\"card\" x=\"0.5\" y=\"0.5\" width=\"" (width - 1) "\" height=\"" (HEIGHT - 1) "\" rx=\"8\"/>")
	svg("  <text class=\"label\" x=\"" PAD "\" y=\"" LABEL_Y "\">PLAY TIME</text>")
	svg("  <text class=\"stat\" x=\"" PAD "\" y=\"" HEADLINE_Y "\">" played "</text>")
	svg("  <text class=\"label\" x=\"" (width - PAD) "\" y=\"" LABEL_Y "\" text-anchor=\"end\">LAST PLAYED" \
			" <tspan class=\"stat\">" last_played "</tspan></text>")
	svg("  <text class=\"note\" x=\"" (width - PAD) "\" y=\"" HEADLINE_Y "\" text-anchor=\"end\">" summary "</text>")

	for (i = 0; i < days; i++) {
		minutes = by_day[days - 1 - i] + 0
		height = (tallest > 0) ? minutes / tallest * CEILING : 0
		# A day too quiet to draw still gets a stub, so the empty days read as an
		# axis rather than as a gap in the chart.
		if (height < MINIMUM_BAR) {
			height = MINIMUM_BAR
			style = (minutes > 0) ? "bar" : "bar-quiet"
		}
		else {
			style = "bar"
		}
		# SVG measures y downwards, so the top of a bar is the baseline minus its height.
		svg(sprintf("  <rect class=\"%s\" x=\"%.1f\" y=\"%.1f\" width=\"%d\" height=\"%.1f\" rx=\"1.5\"/>",
				style, PAD + i * bar_step, BASE - height, bar, height))
	}

	svg("  <text class=\"axis\" x=\"" PAD "\" y=\"" AXIS_Y "\">" first_seen "</text>")
	svg("  <text class=\"axis\" x=\"" (width / 2) "\" y=\"" AXIS_Y "\" text-anchor=\"middle\">" \
			grouped(lived) " " plural(lived, "day") "</text>")
	svg("  <text class=\"axis\" x=\"" (width - PAD) "\" y=\"" AXIS_Y "\" text-anchor=\"end\">" last_seen "</text>")
	svg("</svg>")
	close(card)

	printf "" > block
	markdown("")
	markdown("![Play time](.idea/readme/image/time-on-record.svg)")
	markdown("")
	markdown("<details>")
	markdown("<summary>How this is counted</summary>")
	markdown("")
	markdown("Commits record when work was saved, never how long it took, so this is an")
	markdown("estimate rather than a timesheet. Commits less than " int(gap / 60) " " \
			plural(int(gap / 60), "minute") " apart")
	markdown("count as one sitting and contribute the real time between them; a commit that")
	markdown("opens a sitting contributes a flat " opening " " plural(opening, "minute") \
			" for the work that led up to")
	markdown("it. Merges are skipped, and nothing that was never committed is visible here.")
	markdown("")
	markdown("Covers every author. Regenerated on each commit by `.githooks/coding-time`,")
	markdown("which reads commit timestamps and nothing else. `GAP_MINUTES`, `OPENING_MINUTES`,")
	markdown("`RECENT_DAYS` and `DAYS` change what it assumes.")
	markdown("")
	markdown("</details>")
	markdown("")
	close(block)
}
