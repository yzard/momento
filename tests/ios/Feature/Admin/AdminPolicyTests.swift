import Testing
@testable import Momento

struct AdminPolicyTests {
    @Test func protectsCurrentAndReservedUsers() {
        var user = APIClientTests.session().user
        #expect(!AdminPolicy.canChangeRole(user, currentUserId: user.id))
        #expect(!AdminPolicy.canDeactivateOrDelete(user, currentUserId: user.id))
        #expect(AdminPolicy.canDeactivateOrDelete(user, currentUserId: 99))
        user.isReserved = true
        #expect(AdminPolicy.canChangeRole(user, currentUserId: 99))
        #expect(!AdminPolicy.canDeactivateOrDelete(user, currentUserId: 99))
    }
    @Test func validatesFiveIndependentCronFields() {
        #expect(AdminPolicy.validCron(["0", "2", "*", "*", "*"]))
        #expect(!AdminPolicy.validCron(["0", "2", "*", "*"]))
        #expect(!AdminPolicy.validCron(["0", "", "*", "*", "*"]))
        #expect(!AdminPolicy.validCron(["0 1", "2", "*", "*", "*"]))
    }
    @Test func activeStatesPreventCleaning() {
        for state in ["queued", "submitting", "submitted", "running", "cancelling"] { #expect(AdminPolicy.activeAI(state)) }
        for state in ["idle", "completed", "failed", "cancelled"] { #expect(!AdminPolicy.activeAI(state)) }
    }
}
