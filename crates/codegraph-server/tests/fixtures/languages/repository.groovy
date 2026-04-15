import groovy.sql.Sql

class UserRepository {
    private Sql sql

    UserRepository(Sql sql) {
        this.sql = sql
    }

    Map findById(long id) {
        return sql.firstRow("SELECT * FROM users WHERE id = ?", [id])
    }

    List<Map> findAll() {
        return sql.rows("SELECT * FROM users")
    }

    void save(Map user) {
        sql.execute("INSERT INTO users (name, email) VALUES (?, ?)",
            [user.name, user.email])
    }
}
