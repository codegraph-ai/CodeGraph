package MyApp::User;
use strict;
use warnings;
use Moose;

sub new {
    my ($class, %args) = @_;
    return bless \%args, $class;
}

sub greet {
    my ($self) = @_;
    print "Hello, " . $self->{name} . "\n";
}

sub _validate {
    my ($self) = @_;
    die "Name required" unless $self->{name};
}

1;
