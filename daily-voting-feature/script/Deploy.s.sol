// SPDX-License-Identifier: MIT
pragma solidity ^0.8.19;

import "forge-std/Script.sol";
import "../contracts/DailyVoting.sol";

contract DeployDailyVoting is Script {
    // Token address on BASE
    address constant TOKEN_ADDRESS = 0x587Cd533F418825521f3A1daa7CCd1E7339A1B07;
    
    // Vote price: 1 token (adjust decimals based on token)
    uint256 constant VOTE_PRICE = 1e18;

    function run() external {
        uint256 deployerPrivateKey = vm.envUint("PRIVATE_KEY");
        
        vm.startBroadcast(deployerPrivateKey);

        DailyVoting voting = new DailyVoting(
            TOKEN_ADDRESS,
            VOTE_PRICE
        );

        console.log("DailyVoting deployed to:", address(voting));
        console.log("Token address:", TOKEN_ADDRESS);
        console.log("Vote price:", VOTE_PRICE);

        vm.stopBroadcast();
    }
}

contract AuthorizeBackend is Script {
    function run() external {
        uint256 deployerPrivateKey = vm.envUint("PRIVATE_KEY");
        address votingContract = vm.envAddress("VOTING_CONTRACT");
        address backendAddress = vm.envAddress("BACKEND_ADDRESS");
        
        vm.startBroadcast(deployerPrivateKey);

        DailyVoting voting = DailyVoting(votingContract);
        voting.setAuthorizedBackend(backendAddress, true);

        console.log("Backend authorized:", backendAddress);

        vm.stopBroadcast();
    }
}
