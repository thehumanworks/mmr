# Tech-debt tracking                         
                                                                  
Store each tech-debt finding as a single markdown file in        
`docs/tech-debt/tracked/`.                                       
                                                                  
## Structure                                  
                                              
docs/tech-debt/                 
├── README.md                   
├── TEMPLATE.md                                                  
├── tracked/
└── handled/                                                     
    ├── remediated/                           
    └── dismissed/                           
            
## Creating a debt item         

1. Check `tracked/`, `handled/remediated/`, and                  
`handled/dismissed/` for existing items covering the same issue.
Do not create duplicates. If an existing item partially overlaps,
  extend it instead.                           
2. Copy `TEMPLATE.md`. Fill in all frontmatter fields. Set
`status: needs-triage` and `discovered:` to today's date.        
3. Name the file in kebab-case derived from the title (e.g.,
`missing-input-validation.md`).                                  
4. If the issue is trivial to fix (< 5 minutes, no architectural
decision), fix it directly instead of creating a debt document.  
                                              
## Document rules                                                
                                              
Each debt document must be:                                      
            
- **Single-purpose** — one debt item per file.                   
- **Frontmatter-first** — YAML metadata: `title`, `severity`,
`effort`, `status`, `discovered`, `files`, `related`.            
- **Evidence-based** — remediation plans that propose
architectural changes or non-obvious trade-offs must cite        
sources.                                      
- **Ordered** — list remediation plans by value-to-effort ratio; 
lower-complexity, higher-value plans first.                      
                                              
## Severity definitions                                          
                                              
| Level    | Criteria                                            
              |                               
|----------|-----------------------------------------------------
--------------|                               
| Critical | Data loss risk, security vulnerability, or blocks
shipping.       |
| High     | Measurable correctness or performance impact in
production.       |                                              
| Medium   | Maintainability drag; slows development or increases
  bug risk.    |                                                  
| Low      | Style, naming, minor cleanup. No functional impact.
              |                                                  
            
## Lifecycle                                                     
                                              
needs-triage → approved → in-progress → remediated               
                  ↓
                dismissed                                         
                                              
1. **Discovery.** Create a new file in `tracked/` with `status:
needs-triage`.                                                   
2. **Triage.** Wait for user review. User sets `status: approved`
  or requests dismissal.                                          
3. **Remediation.** Set `status: in-progress`. Implement the fix.
  Fill in the Resolution section. Move the file to                
`handled/remediated/`.                        
4. **Dismissal.** Requires explicit user approval. Fill in the   
Resolution section with detailed reasoning. Move the file to
`handled/dismissed/`.                        
            
## Rules                        

- Do not change `status` from `needs-triage` to `approved` or    
`in-progress` without user consent.
- Do not dismiss without explicit user consent.                  
- Default to remediation. Dismissal is the exception.            
- Fill in the Resolution section before moving any file out of
`tracked/`.                                                      
- Co-locate the document move and the code fix in the same
commit.                                                          
                                              
## Prioritisation                                                
                                              
When the user asks what to work on, sort `tracked/` items by:    
severity descending, then effort ascending, then discovered date
ascending. Present as a table.                                   
                                              
## Staleness                                 
            
When reviewing `tracked/`, flag any item with `status:           
needs-triage` and a `discovered` date older than 90 days. Ask the
  user whether to triage or dismiss.    