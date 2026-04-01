import sys
import json  # For potential output formatting

# Simple keyword-based sentiment analyzer (no external deps needed)
POSITIVE_WORDS = {'good', 'great', 'buy', 'moon', 'bullish', 'up', 'pump', 'awesome', 'win', 'profit'}
NEGATIVE_WORDS = {'bad', 'sell', 'bearish', 'scam', 'down', 'dump', 'lose', 'crash', 'rug', 'fud'}

def calculate_sentiment(text):
    """
    Calculates a sentiment score for the given text.
    Score ranges from -1 (very negative) to 1 (very positive).
    Handles empty text gracefully.
    """
    if not text.strip():
        return 0.0  # Neutral for empty input
    
    words = text.lower().split()
    total_words = len(words)
    if total_words == 0:
        return 0.0
    
    pos_count = sum(1 for word in words if word in POSITIVE_WORDS)
    neg_count = sum(1 for word in words if word in NEGATIVE_WORDS)
    
    # Normalized score: (pos - neg) / total, clamped to [-1, 1]
    score = (pos_count - neg_count) / total_words
    score = max(min(score, 1.0), -1.0)
    
    return score

if __name__ == "__main__":
    try:
        # Read aggregated tweet text from stdin
        input_text = sys.stdin.read().strip()
        
        # Calculate score
        sentiment_score = calculate_sentiment(input_text)
        
        # Output as JSON for easy parsing (e.g., by StarkBot)
        output = {"score": sentiment_score}
        print(json.dumps(output))
    
    except Exception as e:
        # Error handling: Output neutral score and log error
        print(json.dumps({"score": 0.0, "error": str(e)}))
        sys.exit(1)
