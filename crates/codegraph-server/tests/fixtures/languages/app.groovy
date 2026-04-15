import groovy.json.JsonSlurper
import groovy.json.JsonOutput

class Application {
    private UserService userService
    private UserRepository userRepository

    Application(UserService svc, UserRepository repo) {
        this.userService = svc
        this.userRepository = repo
    }

    String handleCreateUser(String jsonBody) {
        def slurper = new JsonSlurper()
        def params = slurper.parseText(jsonBody)
        def user = userService.createUser(params.name, params.email)
        userRepository.save(user)
        return JsonOutput.toJson(user)
    }

    String handleGetUser(long id) {
        def user = userRepository.findById(id)
        if (user == null) {
            return JsonOutput.toJson([error: "not found"])
        }
        return JsonOutput.toJson(user)
    }
}
