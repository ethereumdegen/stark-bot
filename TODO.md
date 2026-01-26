## TODO


### Integration user stories 

1. make the bot be able to tell you about the weather (using teh weather skill and exec tool ) 

tell me about the weather currently in austin texas



2. create a pr on this repo https://github.com/ethereumdegen/vibegraph-rs  that adds 'hello world' to the bottom of the readme 



3. Every 1 hour,  fetch the weather in detroit michigan and  post that to discord.  


4. every 24 hours,  make a report summary of the recent PRs on  https://github.com/clawdbot/clawdbot  and post them in discord 



## Add
add the cron skill and memory skill   





## function calling 


You are an agent named StarkBot who is able to respond and operate tools.   You will often be given a list of tools that you can call.  Always respond in json in the following format: 

{  body: string , tool_call: option< {  tool_name: String, tool_params: Object  } >   }



lets give the tools like using the openai tools schema spec and we should dynamically build it using the actual enabled tools .  one tool will be the 'skills ' tool and it will have nested within it al of the available skills 





### FIx git config..

ow git will automatically use:                                                                                                     
  - Name: StarkBot                                                                                                                           
  - Email: starkbot@users.noreply.github.com                                                                                                 
                                                                                                                                             
  Using environment variables (GIT_AUTHOR_NAME, GIT_AUTHOR_EMAIL, etc.) means no git config commands needed - it just works.                 
                                                                                                                                             
  Restart the backend and try again.                                                                                                         
                                                                                                                                             
  ---                                                                                                                                        
  Future improvement: Make this configurable via:                                                                                            
  - Database settings (API Keys page could have a "Git Identity" section)                                                                    
  - Or environment variables in .env file                                                                                                    
