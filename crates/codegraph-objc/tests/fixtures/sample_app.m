#import <Foundation/Foundation.h>
#import "AppDelegate.h"
#import "UserService.h"

@protocol Validatable <NSObject>
- (BOOL)isValid;
- (NSArray<NSString *> *)validationErrors;
@end

@interface User : NSObject <Validatable>
@property (nonatomic, strong) NSString *name;
@property (nonatomic, strong) NSString *email;
@property (nonatomic, assign) NSInteger age;
- (instancetype)initWithName:(NSString *)name email:(NSString *)email;
- (NSString *)displayName;
+ (User *)userWithName:(NSString *)name email:(NSString *)email;
@end

@implementation User

- (instancetype)initWithName:(NSString *)name email:(NSString *)email {
    self = [super init];
    if (self) {
        _name = name;
        _email = email;
        _age = 0;
    }
    return self;
}

- (NSString *)displayName {
    return [NSString stringWithFormat:@"%@ <%@>", self.name, self.email];
}

+ (User *)userWithName:(NSString *)name email:(NSString *)email {
    return [[User alloc] initWithName:name email:email];
}

- (BOOL)isValid {
    if (self.name.length == 0) return NO;
    if (self.email.length == 0) return NO;
    if (![self.email containsString:@"@"]) return NO;
    return YES;
}

- (NSArray<NSString *> *)validationErrors {
    NSMutableArray *errors = [NSMutableArray array];
    if (self.name.length == 0) {
        [errors addObject:@"Name is required"];
    }
    if (self.email.length == 0) {
        [errors addObject:@"Email is required"];
    } else if (![self.email containsString:@"@"]) {
        [errors addObject:@"Email is invalid"];
    }
    return [errors copy];
}

@end

@interface UserRepository : NSObject
@property (nonatomic, strong) NSMutableArray<User *> *users;
- (void)addUser:(User *)user;
- (User *)findUserByEmail:(NSString *)email;
- (NSArray<User *> *)allUsers;
- (void)removeUser:(User *)user;
@end

@implementation UserRepository

- (instancetype)init {
    self = [super init];
    if (self) {
        _users = [NSMutableArray array];
    }
    return self;
}

- (void)addUser:(User *)user {
    if ([user isValid]) {
        [self.users addObject:user];
        NSLog(@"Added user: %@", [user displayName]);
    }
}

- (User *)findUserByEmail:(NSString *)email {
    for (User *user in self.users) {
        if ([user.email isEqualToString:email]) {
            return user;
        }
    }
    return nil;
}

- (NSArray<User *> *)allUsers {
    return [self.users copy];
}

- (void)removeUser:(User *)user {
    [self.users removeObject:user];
}

@end

@interface AppController : NSObject
@property (nonatomic, strong) UserRepository *repository;
- (void)registerUserWithName:(NSString *)name email:(NSString *)email;
- (void)listUsers;
@end

@implementation AppController

- (instancetype)init {
    self = [super init];
    if (self) {
        _repository = [[UserRepository alloc] init];
    }
    return self;
}

- (void)registerUserWithName:(NSString *)name email:(NSString *)email {
    User *user = [User userWithName:name email:email];
    if ([user isValid]) {
        [self.repository addUser:user];
    } else {
        NSLog(@"Invalid user: %@", [user validationErrors]);
    }
}

- (void)listUsers {
    NSArray *users = [self.repository allUsers];
    for (User *user in users) {
        NSLog(@"%@", [user displayName]);
    }
}

@end
