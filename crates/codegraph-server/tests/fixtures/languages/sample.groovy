import groovy.json.JsonSlurper

class UserService {
    private String baseUrl

    def createUser(String name, String email) {
        return [name: name, email: email]
    }

    private void validate(Map user) {
        if (user.name.isEmpty()) {
            throw new IllegalArgumentException("Name required")
        }
    }
}
