// SPDX-License-Identifier: MIT
pragma solidity ^0.8.19;

import "forge-std/Test.sol";
import "../contracts/DailyVoting.sol";
import "@openzeppelin/contracts/token/ERC20/ERC20.sol";

// Mock ERC20 token for testing
contract MockToken is ERC20 {
    constructor() ERC20("Mock Token", "MOCK") {
        _mint(msg.sender, 1_000_000 * 10**18);
    }

    function mint(address to, uint256 amount) external {
        _mint(to, amount);
    }
}

contract DailyVotingTest is Test {
    DailyVoting public voting;
    MockToken public token;
    
    address public owner = address(this);
    address public backend = address(0x1);
    address public creator1 = address(0x2);
    address public creator2 = address(0x3);
    address public voter1 = address(0x4);
    address public voter2 = address(0x5);
    address public voter3 = address(0x6);
    
    uint256 public constant VOTE_PRICE = 10 * 10**18; // 10 tokens
    
    bytes32 public post1Id = keccak256("post_1");
    bytes32 public post2Id = keccak256("post_2");

    function setUp() public {
        // Deploy mock token
        token = new MockToken();
        
        // Deploy voting contract
        voting = new DailyVoting(address(token), VOTE_PRICE);
        
        // Authorize backend
        voting.setAuthorizedBackend(backend, true);
        
        // Distribute tokens to voters
        token.mint(voter1, 1000 * 10**18);
        token.mint(voter2, 1000 * 10**18);
        token.mint(voter3, 1000 * 10**18);
        
        // Approve voting contract
        vm.prank(voter1);
        token.approve(address(voting), type(uint256).max);
        vm.prank(voter2);
        token.approve(address(voting), type(uint256).max);
        vm.prank(voter3);
        token.approve(address(voting), type(uint256).max);
    }

    function test_InitialState() public {
        assertEq(voting.currentCycleId(), 1);
        assertEq(voting.votePrice(), VOTE_PRICE);
        assertEq(address(voting.votingToken()), address(token));
    }

    function test_RegisterPost() public {
        vm.prank(backend);
        voting.registerPost(post1Id, creator1);
        
        (address creator, uint256 votes, uint256 staked) = voting.getPost(1, post1Id);
        assertEq(creator, creator1);
        assertEq(votes, 0);
        assertEq(staked, 0);
    }

    function test_RegisterPost_Unauthorized() public {
        vm.prank(voter1);
        vm.expectRevert(DailyVoting.UnauthorizedBackend.selector);
        voting.registerPost(post1Id, creator1);
    }

    function test_Vote() public {
        // Register post
        vm.prank(backend);
        voting.registerPost(post1Id, creator1);
        
        // Cast vote
        vm.prank(voter1);
        voting.vote(post1Id, VOTE_PRICE);
        
        // Check post stats
        (address creator, uint256 votes, uint256 staked) = voting.getPost(1, post1Id);
        assertEq(creator, creator1);
        assertEq(votes, 1);
        assertEq(staked, VOTE_PRICE);
        
        // Check cycle pool
        (,,, uint256 totalPool,,) = voting.getCurrentCycle();
        assertEq(totalPool, VOTE_PRICE);
    }

    function test_Vote_InsufficientAmount() public {
        vm.prank(backend);
        voting.registerPost(post1Id, creator1);
        
        vm.prank(voter1);
        vm.expectRevert(DailyVoting.InsufficientVoteAmount.selector);
        voting.vote(post1Id, VOTE_PRICE - 1);
    }

    function test_Vote_AlreadyVoted() public {
        vm.prank(backend);
        voting.registerPost(post1Id, creator1);
        
        vm.prank(voter1);
        voting.vote(post1Id, VOTE_PRICE);
        
        // Wait for cooldown
        vm.warp(block.timestamp + 2 minutes);
        
        vm.prank(voter1);
        vm.expectRevert(DailyVoting.AlreadyVotedForPost.selector);
        voting.vote(post1Id, VOTE_PRICE);
    }

    function test_Vote_Cooldown() public {
        vm.prank(backend);
        voting.registerPost(post1Id, creator1);
        voting.registerPost(post2Id, creator2);
        
        vm.prank(voter1);
        voting.vote(post1Id, VOTE_PRICE);
        
        // Try to vote again immediately
        vm.prank(voter1);
        vm.expectRevert(DailyVoting.VoteCooldown.selector);
        voting.vote(post2Id, VOTE_PRICE);
    }

    function test_FinalizeCycle() public {
        // Register posts
        vm.prank(backend);
        voting.registerPost(post1Id, creator1);
        vm.prank(backend);
        voting.registerPost(post2Id, creator2);
        
        // Cast votes - post1 wins with 2 votes
        vm.prank(voter1);
        voting.vote(post1Id, VOTE_PRICE);
        
        vm.warp(block.timestamp + 2 minutes);
        vm.prank(voter2);
        voting.vote(post1Id, VOTE_PRICE * 2); // 20 tokens
        
        vm.warp(block.timestamp + 2 minutes);
        vm.prank(voter3);
        voting.vote(post2Id, VOTE_PRICE);
        
        // Fast forward past cycle end
        vm.warp(block.timestamp + 25 hours);
        
        // Finalize
        voting.finalizeCycle();
        
        // Check winner
        (,,,, bytes32 winner,) = voting.cycles(1);
        assertEq(winner, post1Id);
        
        // New cycle should be created
        assertEq(voting.currentCycleId(), 2);
    }

    function test_RewardDistribution() public {
        // Setup
        vm.prank(backend);
        voting.registerPost(post1Id, creator1);
        
        vm.prank(voter1);
        voting.vote(post1Id, 100 * 10**18);
        
        uint256 creatorBalanceBefore = token.balanceOf(creator1);
        uint256 burnAddressBalanceBefore = token.balanceOf(voting.BURN_ADDRESS());
        
        // Fast forward and finalize
        vm.warp(block.timestamp + 25 hours);
        voting.finalizeCycle();
        
        // Check creator got 30%
        uint256 expectedCreatorReward = (100 * 10**18 * 30) / 100;
        assertEq(token.balanceOf(creator1) - creatorBalanceBefore, expectedCreatorReward);
        
        // Check burn address got 20%
        uint256 expectedBurn = (100 * 10**18 * 20) / 100;
        assertEq(token.balanceOf(voting.BURN_ADDRESS()) - burnAddressBalanceBefore, expectedBurn);
        
        // Check voter reward allocated (50%)
        (uint256 voterReward, bool claimed) = voting.getPendingReward(1, voter1);
        uint256 expectedVoterReward = (100 * 10**18 * 50) / 100;
        assertEq(voterReward, expectedVoterReward);
        assertFalse(claimed);
    }

    function test_ClaimReward() public {
        // Setup and finalize
        vm.prank(backend);
        voting.registerPost(post1Id, creator1);
        
        vm.prank(voter1);
        voting.vote(post1Id, 100 * 10**18);
        
        vm.warp(block.timestamp + 25 hours);
        voting.finalizeCycle();
        
        // Claim reward
        uint256 balanceBefore = token.balanceOf(voter1);
        
        vm.prank(voter1);
        voting.claimReward(1);
        
        uint256 expectedReward = (100 * 10**18 * 50) / 100;
        assertEq(token.balanceOf(voter1) - balanceBefore, expectedReward);
        
        // Check claimed status
        (uint256 amount, bool claimed) = voting.getPendingReward(1, voter1);
        assertTrue(claimed);
    }

    function test_GetLeaderboard() public {
        // Register posts
        vm.prank(backend);
        voting.registerPost(post1Id, creator1);
        vm.prank(backend);
        voting.registerPost(post2Id, creator2);
        
        // Vote - post1 gets more
        vm.prank(voter1);
        voting.vote(post1Id, VOTE_PRICE * 3);
        
        vm.warp(block.timestamp + 2 minutes);
        vm.prank(voter2);
        voting.vote(post2Id, VOTE_PRICE);
        
        // Get leaderboard
        (bytes32[] memory postIds, uint256[] memory votes, uint256[] memory stakes) = 
            voting.getLeaderboard(1, 10);
        
        assertEq(postIds.length, 2);
        assertEq(postIds[0], post1Id); // post1 should be first
        assertEq(votes[0], 1);
        assertEq(stakes[0], VOTE_PRICE * 3);
    }

    function test_Pause() public {
        voting.pause();
        
        vm.prank(backend);
        vm.expectRevert("Pausable: paused");
        voting.registerPost(post1Id, creator1);
        
        voting.unpause();
        
        vm.prank(backend);
        voting.registerPost(post1Id, creator1); // Should work now
    }
}
