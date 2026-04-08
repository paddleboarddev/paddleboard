use strict;
use warnings;

my $file = 'crates/install_cli/src/install_cli_binary.rs';
open my $in, '<', $file or die $!;
my @lines = <$in>;
close $in;

open my $out, '>', $file or die $!;
for my $line (@lines) {
    if ($line =~ /"Installed `paddleboard` to \{\}\. You can launch \{\} from your terminal\.",/) {
        print $out '                        "Installed `paddleboard` to {}. You can launch PaddleBoard from your terminal.",' . "\n";
    } elsif ($line =~ /ReleaseChannel::global\(cx\)\.display_name\(\)/) {
        # remove this line since we hardcoded PaddleBoard in the format string above
    } else {
        print $out $line;
    }
}
close $out;
