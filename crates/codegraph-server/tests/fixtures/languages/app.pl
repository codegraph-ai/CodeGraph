package MyApp::App;
use strict;
use warnings;
use MyApp::User;
use MyApp::Database;

sub new {
    my ($class, %args) = @_;
    my $db = MyApp::Database->new(dsn => $args{dsn}, user => $args{user}, pass => $args{pass});
    return bless { db => $db }, $class;
}

sub create_user {
    my ($self, $name, $email) = @_;
    my $user = MyApp::User->new(name => $name, email => $email);
    $self->{db}->insert("users", name => $name, email => $email);
    return $user;
}

sub get_users {
    my ($self) = @_;
    return $self->{db}->query("SELECT * FROM users");
}

sub run {
    my ($self) = @_;
    my $user = $self->create_user("Alice", "alice\@example.com");
    $user->greet;
    my $users = $self->get_users;
    print "Total users: " . scalar(@$users) . "\n";
}

1;
