// SPDX-License-Identifier: MIT
pragma solidity ^0.8.19;

import "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/security/ReentrancyGuard.sol";
import "@openzeppelin/contracts/security/Pausable.sol";

/**
 * @title DailyVoting
 * @notice Daily post voting system with token rewards for x402book
 * @dev Implements 24-hour voting cycles with reward distribution
 * 
 * Reward Distribution:
 * - 30% to winning post creator
 * - 50% to voters who voted for the winning post
 * - 20% burned (sent to dead address)
 */
contract DailyVoting is Ownable, ReentrancyGuard, Pausable {
    using SafeERC20 for IERC20;

    // ============ Constants ============
    
    address public constant BURN_ADDRESS = 0x000000000000000000000000000000000000dEaD;
    
    uint256 public constant CREATOR_SHARE = 30;   // 30%
    uint256 public constant VOTER_SHARE = 50;     // 50%
    uint256 public constant BURN_SHARE = 20;      // 20%
    uint256 public constant PERCENTAGE_BASE = 100;
    
    uint256 public constant CYCLE_DURATION = 24 hours;
    uint256 public constant MIN_VOTE_AMOUNT = 1e18; // 1 token minimum

    // ============ State Variables ============
    
    IERC20 public immutable votingToken;
    
    uint256 public currentCycleId;
    uint256 public currentCycleStart;
    uint256 public votePrice;
    
    // ============ Structs ============
    
    struct Post {
        bytes32 postId;          // External post identifier (from x402book)
        address creator;         // Post creator address
        uint256 totalVotes;      // Total vote count
        uint256 totalStaked;     // Total tokens staked on this post
        bool exists;
    }
    
    struct Vote {
        address voter;
        bytes32 postId;
        uint256 amount;
        uint256 timestamp;
    }
    
    struct Cycle {
        uint256 id;
        uint256 startTime;
        uint256 endTime;
        uint256 totalPool;       // Total tokens in the cycle pool
        bytes32 winningPostId;
        bool finalized;
        bool rewardsDistributed;
    }
    
    struct VoterReward {
        uint256 amount;
        bool claimed;
    }

    // ============ Mappings ============
    
    // cycleId => Cycle
    mapping(uint256 => Cycle) public cycles;
    
    // cycleId => postId => Post
    mapping(uint256 => mapping(bytes32 => Post)) public cyclePosts;
    
    // cycleId => array of postIds
    mapping(uint256 => bytes32[]) public cyclePostIds;
    
    // cycleId => voter => postId => Vote
    mapping(uint256 => mapping(address => mapping(bytes32 => Vote))) public votes;
    
    // cycleId => voter => array of postIds voted on
    mapping(uint256 => mapping(address => bytes32[])) public voterPosts;
    
    // cycleId => postId => array of voters
    mapping(uint256 => mapping(bytes32 => address[])) public postVoters;
    
    // cycleId => voter => VoterReward
    mapping(uint256 => mapping(address => VoterReward)) public voterRewards;
    
    // Anti-sybil: address => last vote timestamp
    mapping(address => uint256) public lastVoteTime;
    
    // Authorized backends that can register posts
    mapping(address => bool) public authorizedBackends;

    // ============ Events ============
    
    event CycleStarted(uint256 indexed cycleId, uint256 startTime, uint256 endTime);
    event PostRegistered(uint256 indexed cycleId, bytes32 indexed postId, address indexed creator);
    event VoteCast(uint256 indexed cycleId, bytes32 indexed postId, address indexed voter, uint256 amount);
    event CycleFinalized(uint256 indexed cycleId, bytes32 indexed winningPostId, uint256 totalPool);
    event CreatorRewarded(uint256 indexed cycleId, address indexed creator, uint256 amount);
    event VoterRewardAllocated(uint256 indexed cycleId, address indexed voter, uint256 amount);
    event VoterRewardClaimed(uint256 indexed cycleId, address indexed voter, uint256 amount);
    event TokensBurned(uint256 indexed cycleId, uint256 amount);
    event VotePriceUpdated(uint256 oldPrice, uint256 newPrice);
    event BackendAuthorized(address indexed backend, bool authorized);

    // ============ Errors ============
    
    error CycleNotActive();
    error CycleNotEnded();
    error CycleAlreadyFinalized();
    error PostNotFound();
    error PostAlreadyExists();
    error InsufficientVoteAmount();
    error AlreadyVotedForPost();
    error NoRewardToClaim();
    error RewardAlreadyClaimed();
    error UnauthorizedBackend();
    error VoteCooldown();
    error InvalidPostId();
    error InvalidCreator();
    error ZeroAddress();

    // ============ Modifiers ============
    
    modifier onlyAuthorizedBackend() {
        if (!authorizedBackends[msg.sender] && msg.sender != owner()) {
            revert UnauthorizedBackend();
        }
        _;
    }
    
    modifier cycleActive() {
        if (block.timestamp >= currentCycleStart + CYCLE_DURATION) {
            revert CycleNotActive();
        }
        _;
    }

    // ============ Constructor ============
    
    constructor(address _votingToken, uint256 _votePrice) {
        if (_votingToken == address(0)) revert ZeroAddress();
        
        votingToken = IERC20(_votingToken);
        votePrice = _votePrice;
        
        // Start the first cycle
        _startNewCycle();
    }

    // ============ External Functions ============
    
    /**
     * @notice Register a post for the current voting cycle
     * @param postId External post identifier from x402book
     * @param creator Address of the post creator
     */
    function registerPost(bytes32 postId, address creator) 
        external 
        onlyAuthorizedBackend 
        cycleActive 
        whenNotPaused 
    {
        if (postId == bytes32(0)) revert InvalidPostId();
        if (creator == address(0)) revert InvalidCreator();
        if (cyclePosts[currentCycleId][postId].exists) revert PostAlreadyExists();
        
        cyclePosts[currentCycleId][postId] = Post({
            postId: postId,
            creator: creator,
            totalVotes: 0,
            totalStaked: 0,
            exists: true
        });
        
        cyclePostIds[currentCycleId].push(postId);
        
        emit PostRegistered(currentCycleId, postId, creator);
    }
    
    /**
     * @notice Cast a vote for a post
     * @param postId The post to vote for
     * @param amount Amount of tokens to stake (must be >= votePrice)
     */
    function vote(bytes32 postId, uint256 amount) 
        external 
        nonReentrant 
        cycleActive 
        whenNotPaused 
    {
        if (amount < votePrice) revert InsufficientVoteAmount();
        if (!cyclePosts[currentCycleId][postId].exists) revert PostNotFound();
        if (votes[currentCycleId][msg.sender][postId].amount > 0) revert AlreadyVotedForPost();
        
        // Anti-sybil: 1 minute cooldown between votes
        if (block.timestamp < lastVoteTime[msg.sender] + 1 minutes) revert VoteCooldown();
        
        // Transfer tokens from voter
        votingToken.safeTransferFrom(msg.sender, address(this), amount);
        
        // Record the vote
        votes[currentCycleId][msg.sender][postId] = Vote({
            voter: msg.sender,
            postId: postId,
            amount: amount,
            timestamp: block.timestamp
        });
        
        voterPosts[currentCycleId][msg.sender].push(postId);
        postVoters[currentCycleId][postId].push(msg.sender);
        
        // Update post stats
        cyclePosts[currentCycleId][postId].totalVotes += 1;
        cyclePosts[currentCycleId][postId].totalStaked += amount;
        
        // Update cycle pool
        cycles[currentCycleId].totalPool += amount;
        
        // Update last vote time
        lastVoteTime[msg.sender] = block.timestamp;
        
        emit VoteCast(currentCycleId, postId, msg.sender, amount);
    }
    
    /**
     * @notice Finalize the current cycle and determine the winner
     * @dev Can only be called after the cycle has ended
     */
    function finalizeCycle() external nonReentrant whenNotPaused {
        Cycle storage cycle = cycles[currentCycleId];
        
        if (block.timestamp < cycle.endTime) revert CycleNotEnded();
        if (cycle.finalized) revert CycleAlreadyFinalized();
        
        bytes32[] memory postIds = cyclePostIds[currentCycleId];
        bytes32 winningPostId;
        uint256 highestVotes = 0;
        
        // Find the winning post (most votes)
        for (uint256 i = 0; i < postIds.length; i++) {
            Post storage post = cyclePosts[currentCycleId][postIds[i]];
            if (post.totalVotes > highestVotes) {
                highestVotes = post.totalVotes;
                winningPostId = postIds[i];
            }
        }
        
        cycle.winningPostId = winningPostId;
        cycle.finalized = true;
        
        emit CycleFinalized(currentCycleId, winningPostId, cycle.totalPool);
        
        // Distribute rewards if there was a winner
        if (winningPostId != bytes32(0) && cycle.totalPool > 0) {
            _distributeRewards(currentCycleId);
        }
        
        // Start new cycle
        _startNewCycle();
    }
    
    /**
     * @notice Claim voter rewards for a finalized cycle
     * @param cycleId The cycle to claim rewards from
     */
    function claimReward(uint256 cycleId) external nonReentrant whenNotPaused {
        VoterReward storage reward = voterRewards[cycleId][msg.sender];
        
        if (reward.amount == 0) revert NoRewardToClaim();
        if (reward.claimed) revert RewardAlreadyClaimed();
        
        reward.claimed = true;
        
        votingToken.safeTransfer(msg.sender, reward.amount);
        
        emit VoterRewardClaimed(cycleId, msg.sender, reward.amount);
    }

    // ============ View Functions ============
    
    /**
     * @notice Get current cycle information
     */
    function getCurrentCycle() external view returns (
        uint256 cycleId,
        uint256 startTime,
        uint256 endTime,
        uint256 totalPool,
        uint256 timeRemaining,
        bool isActive
    ) {
        Cycle storage cycle = cycles[currentCycleId];
        uint256 remaining = block.timestamp < cycle.endTime 
            ? cycle.endTime - block.timestamp 
            : 0;
            
        return (
            cycle.id,
            cycle.startTime,
            cycle.endTime,
            cycle.totalPool,
            remaining,
            block.timestamp < cycle.endTime
        );
    }
    
    /**
     * @notice Get all posts for a cycle
     */
    function getCyclePosts(uint256 cycleId) external view returns (bytes32[] memory) {
        return cyclePostIds[cycleId];
    }
    
    /**
     * @notice Get post details
     */
    function getPost(uint256 cycleId, bytes32 postId) external view returns (
        address creator,
        uint256 totalVotes,
        uint256 totalStaked
    ) {
        Post storage post = cyclePosts[cycleId][postId];
        return (post.creator, post.totalVotes, post.totalStaked);
    }
    
    /**
     * @notice Get leaderboard (top posts by votes)
     */
    function getLeaderboard(uint256 cycleId, uint256 limit) external view returns (
        bytes32[] memory postIds,
        uint256[] memory voteCounts,
        uint256[] memory stakedAmounts
    ) {
        bytes32[] memory allPosts = cyclePostIds[cycleId];
        uint256 count = limit < allPosts.length ? limit : allPosts.length;
        
        postIds = new bytes32[](count);
        voteCounts = new uint256[](count);
        stakedAmounts = new uint256[](count);
        
        // Simple selection sort for top N (gas efficient for small limits)
        bool[] memory used = new bool[](allPosts.length);
        
        for (uint256 i = 0; i < count; i++) {
            uint256 maxVotes = 0;
            uint256 maxIndex = 0;
            
            for (uint256 j = 0; j < allPosts.length; j++) {
                if (!used[j]) {
                    Post storage post = cyclePosts[cycleId][allPosts[j]];
                    if (post.totalVotes > maxVotes) {
                        maxVotes = post.totalVotes;
                        maxIndex = j;
                    }
                }
            }
            
            used[maxIndex] = true;
            Post storage selectedPost = cyclePosts[cycleId][allPosts[maxIndex]];
            postIds[i] = allPosts[maxIndex];
            voteCounts[i] = selectedPost.totalVotes;
            stakedAmounts[i] = selectedPost.totalStaked;
        }
        
        return (postIds, voteCounts, stakedAmounts);
    }
    
    /**
     * @notice Check if user has voted for a specific post
     */
    function hasVoted(uint256 cycleId, address voter, bytes32 postId) external view returns (bool) {
        return votes[cycleId][voter][postId].amount > 0;
    }
    
    /**
     * @notice Get voter's pending reward for a cycle
     */
    function getPendingReward(uint256 cycleId, address voter) external view returns (uint256 amount, bool claimed) {
        VoterReward storage reward = voterRewards[cycleId][voter];
        return (reward.amount, reward.claimed);
    }

    // ============ Admin Functions ============
    
    /**
     * @notice Update the vote price
     */
    function setVotePrice(uint256 newPrice) external onlyOwner {
        if (newPrice < MIN_VOTE_AMOUNT) revert InsufficientVoteAmount();
        
        uint256 oldPrice = votePrice;
        votePrice = newPrice;
        
        emit VotePriceUpdated(oldPrice, newPrice);
    }
    
    /**
     * @notice Authorize or revoke a backend address
     */
    function setAuthorizedBackend(address backend, bool authorized) external onlyOwner {
        if (backend == address(0)) revert ZeroAddress();
        
        authorizedBackends[backend] = authorized;
        
        emit BackendAuthorized(backend, authorized);
    }
    
    /**
     * @notice Pause the contract
     */
    function pause() external onlyOwner {
        _pause();
    }
    
    /**
     * @notice Unpause the contract
     */
    function unpause() external onlyOwner {
        _unpause();
    }
    
    /**
     * @notice Emergency withdraw (only if contract is paused)
     */
    function emergencyWithdraw(address to, uint256 amount) external onlyOwner whenPaused {
        if (to == address(0)) revert ZeroAddress();
        votingToken.safeTransfer(to, amount);
    }

    // ============ Internal Functions ============
    
    function _startNewCycle() internal {
        currentCycleId++;
        currentCycleStart = block.timestamp;
        
        cycles[currentCycleId] = Cycle({
            id: currentCycleId,
            startTime: block.timestamp,
            endTime: block.timestamp + CYCLE_DURATION,
            totalPool: 0,
            winningPostId: bytes32(0),
            finalized: false,
            rewardsDistributed: false
        });
        
        emit CycleStarted(currentCycleId, block.timestamp, block.timestamp + CYCLE_DURATION);
    }
    
    function _distributeRewards(uint256 cycleId) internal {
        Cycle storage cycle = cycles[cycleId];
        
        if (cycle.rewardsDistributed) return;
        cycle.rewardsDistributed = true;
        
        uint256 totalPool = cycle.totalPool;
        bytes32 winningPostId = cycle.winningPostId;
        Post storage winningPost = cyclePosts[cycleId][winningPostId];
        
        // Calculate shares
        uint256 creatorAmount = (totalPool * CREATOR_SHARE) / PERCENTAGE_BASE;
        uint256 voterPoolAmount = (totalPool * VOTER_SHARE) / PERCENTAGE_BASE;
        uint256 burnAmount = (totalPool * BURN_SHARE) / PERCENTAGE_BASE;
        
        // Send creator reward
        if (creatorAmount > 0 && winningPost.creator != address(0)) {
            votingToken.safeTransfer(winningPost.creator, creatorAmount);
            emit CreatorRewarded(cycleId, winningPost.creator, creatorAmount);
        }
        
        // Burn tokens
        if (burnAmount > 0) {
            votingToken.safeTransfer(BURN_ADDRESS, burnAmount);
            emit TokensBurned(cycleId, burnAmount);
        }
        
        // Allocate voter rewards (proportional to their stake on winning post)
        address[] memory winners = postVoters[cycleId][winningPostId];
        uint256 totalWinningStake = winningPost.totalStaked;
        
        if (totalWinningStake > 0 && voterPoolAmount > 0) {
            for (uint256 i = 0; i < winners.length; i++) {
                address voter = winners[i];
                Vote storage voterVote = votes[cycleId][voter][winningPostId];
                
                // Proportional reward based on stake
                uint256 voterShare = (voterVote.amount * voterPoolAmount) / totalWinningStake;
                
                if (voterShare > 0) {
                    voterRewards[cycleId][voter] = VoterReward({
                        amount: voterShare,
                        claimed: false
                    });
                    
                    emit VoterRewardAllocated(cycleId, voter, voterShare);
                }
            }
        }
    }
}
