package MyApp::Database;
use strict;
use warnings;
use DBI;

sub new {
    my ($class, %args) = @_;
    my $dbh = DBI->connect($args{dsn}, $args{user}, $args{pass});
    return bless { dbh => $dbh }, $class;
}

sub query {
    my ($self, $sql, @params) = @_;
    my $sth = $self->{dbh}->prepare($sql);
    $sth->execute(@params);
    return $sth->fetchall_arrayref({});
}

sub insert {
    my ($self, $table, %data) = @_;
    my @cols = keys %data;
    my @vals = values %data;
    my $placeholders = join(", ", ("?") x scalar @cols);
    my $cols_str = join(", ", @cols);
    $self->{dbh}->do("INSERT INTO $table ($cols_str) VALUES ($placeholders)", undef, @vals);
}

sub disconnect {
    my ($self) = @_;
    $self->{dbh}->disconnect if $self->{dbh};
}

1;
