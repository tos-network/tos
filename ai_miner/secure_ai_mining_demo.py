#!/usr/bin/env python3
"""
TOS AI Mining - Secure Economic Model Demo
Secure AI mining economic model - prevent free quota attacks
"""

import json
import urllib.request
import time
import hashlib
import random
import sys
from datetime import datetime

class SecureTOSAIMiningDemo:
    def __init__(self, daemon_url: str = "http://127.0.0.1:8080"):
        self.daemon_url = daemon_url

    def calculate_secure_gas_pricing(self, content_length: int, difficulty: str, account_reputation: float, stake_amount: int) -> dict:
        """Secure gas cost calculation model"""

        # Base fee
        base_fee = 2500  # 2500 nanoTOS

        # 1. Minimum economic threshold (prevent spam attacks)
        MIN_COST_PER_TX = 50_000  # 0.00005 TOS minimum fee

        # 2. Stake requirement coefficient
        stake_multiplier = 1.0
        if stake_amount < 10_000:  # Less than 0.00001 TOS stake
            stake_multiplier = 5.0  # Increase fee by 5x
        elif stake_amount < 100_000:  # Less than 0.0001 TOS stake
            stake_multiplier = 2.0  # Increase fee by 2x

        # 3. Reputation discount coefficient (high reputation users get discounts)
        reputation_discount = 1.0
        if account_reputation >= 0.9:
            reputation_discount = 0.5  # 50% discount
        elif account_reputation >= 0.7:
            reputation_discount = 0.7  # 30% discount
        elif account_reputation < 0.3:
            reputation_discount = 2.0  # Low reputation users pay increased fees

        # 4. Content storage cost - progressive pricing
        if content_length <= 50:
            # Ultra-short content: minimum fee but not free
            content_cost = MIN_COST_PER_TX
        elif content_length <= 200:
            # Short content: lower fee to encourage participation
            content_cost = content_length * 500  # 0.0000005 TOS/byte
        elif content_length <= 1000:
            # Medium content: standard fee
            content_cost = 200 * 500 + (content_length - 200) * 1000  # 0.000001 TOS/byte
        else:
            # Long content: progressive fee
            content_cost = (
                200 * 500 +  # First 200 bytes
                800 * 1000 +  # 200-1000 bytes
                (content_length - 1000) * 2000  # 1000+ bytes
            )

        # 5. Difficulty coefficient
        difficulty_multipliers = {
            "Basic": 1.0,
            "Intermediate": 1.2,
            "Advanced": 1.5,
            "Expert": 2.0
        }
        difficulty_multiplier = difficulty_multipliers.get(difficulty, 1.0)

        # 6. Final cost calculation
        raw_cost = base_fee + content_cost * difficulty_multiplier
        adjusted_cost = int(raw_cost * stake_multiplier * reputation_discount)

        # 7. Ensure minimum fee
        final_cost = max(adjusted_cost, MIN_COST_PER_TX)

        return {
            "base_fee": base_fee,
            "content_cost": content_cost,
            "difficulty_multiplier": difficulty_multiplier,
            "stake_multiplier": stake_multiplier,
            "reputation_discount": reputation_discount,
            "min_cost": MIN_COST_PER_TX,
            "final_cost": final_cost,
            "cost_breakdown": {
                "base": base_fee,
                "content": content_cost,
                "difficulty_adj": content_cost * difficulty_multiplier,
                "stake_adj": raw_cost * stake_multiplier,
                "reputation_adj": raw_cost * stake_multiplier * reputation_discount
            }
        }

    def calculate_anti_sybil_measures(self, account_age_days: int, tx_history_count: int, stake_amount: int) -> dict:
        """Anti-Sybil attack measures"""

        # 1. Account age requirement
        age_score = min(account_age_days / 30.0, 1.0)  # 30 days to reach max score

        # 2. Transaction history requirement
        history_score = min(tx_history_count / 100.0, 1.0)  # 100 transactions to reach max score

        # 3. Stake requirement
        stake_score = min(stake_amount / 1_000_000, 1.0)  # 0.001 TOS to reach max score

        # 4. Comprehensive reputation score
        reputation = (age_score * 0.3 + history_score * 0.4 + stake_score * 0.3)

        # 5. Access thresholds
        MIN_REPUTATION_FOR_BASIC = 0.1
        MIN_REPUTATION_FOR_INTERMEDIATE = 0.3
        MIN_REPUTATION_FOR_ADVANCED = 0.5
        MIN_REPUTATION_FOR_EXPERT = 0.7

        access_levels = []
        if reputation >= MIN_REPUTATION_FOR_BASIC:
            access_levels.append("Basic")
        if reputation >= MIN_REPUTATION_FOR_INTERMEDIATE:
            access_levels.append("Intermediate")
        if reputation >= MIN_REPUTATION_FOR_ADVANCED:
            access_levels.append("Advanced")
        if reputation >= MIN_REPUTATION_FOR_EXPERT:
            access_levels.append("Expert")

        return {
            "age_score": age_score,
            "history_score": history_score,
            "stake_score": stake_score,
            "reputation": reputation,
            "access_levels": access_levels,
            "min_stake_required": 100_000 if reputation < 0.5 else 50_000
        }

    def calculate_rate_limiting(self, account_reputation: float, last_submission_time: int) -> dict:
        """Rate limiting mechanism"""

        current_time = int(time.time())
        time_since_last = current_time - last_submission_time

        # Reputation-based cooldown time
        if account_reputation >= 0.8:
            cooldown_period = 300    # 5 minutes
        elif account_reputation >= 0.5:
            cooldown_period = 900    # 15 minutes
        elif account_reputation >= 0.3:
            cooldown_period = 1800   # 30 minutes
        else:
            cooldown_period = 3600   # 1 hour

        can_submit = time_since_last >= cooldown_period
        remaining_cooldown = max(0, cooldown_period - time_since_last)

        return {
            "can_submit": can_submit,
            "cooldown_period": cooldown_period,
            "remaining_cooldown": remaining_cooldown,
            "next_submission_time": current_time + remaining_cooldown if not can_submit else current_time
        }

    def run_security_analysis(self):
        """Run security analysis demo"""
        print("🔒 TOS AI Mining - Secure Economic Model Demo")
        print("🛡️ Security economic model analysis to prevent attacks")
        print("⏱️ Start time:", datetime.now().strftime("%Y-%m-%d %H:%M:%S"))

        print("\n" + "="*80)
        print("🚨 Attack scenario analysis")
        print("="*80)

        # Simulate different types of attackers
        attackers = [
            {
                "name": "Spam content attacker",
                "account_age_days": 1,
                "tx_history_count": 0,
                "stake_amount": 0,
                "content_length": 99,
                "difficulty": "Basic",
                "description": "Attempts to attack with 99 bytes of spam content"
            },
            {
                "name": "Sybil attacker",
                "account_age_days": 0,
                "tx_history_count": 0,
                "stake_amount": 1000,
                "content_length": 50,
                "difficulty": "Basic",
                "description": "Bulk creation of new accounts for attacks"
            },
            {
                "name": "Low-quality content farm",
                "account_age_days": 7,
                "tx_history_count": 5,
                "stake_amount": 10000,
                "content_length": 200,
                "difficulty": "Intermediate",
                "description": "Mass production of low-quality content"
            },
            {
                "name": "Normal new user",
                "account_age_days": 3,
                "tx_history_count": 2,
                "stake_amount": 50000,
                "content_length": 300,
                "difficulty": "Basic",
                "description": "Normal novice user"
            },
            {
                "name": "High reputation user",
                "account_age_days": 90,
                "tx_history_count": 150,
                "stake_amount": 2000000,
                "content_length": 800,
                "difficulty": "Advanced",
                "description": "Long-term high-quality contributor"
            }
        ]

        print(f"{'User Type':<20} {'Reputation':<8} {'Access Level':<15} {'Fee(nanoTOS)':<15} {'Cooldown':<12} {'Protection Effect':<20}")
        print("-" * 100)

        for attacker in attackers:
            # Calculate anti-Sybil measures
            sybil_measures = self.calculate_anti_sybil_measures(
                attacker["account_age_days"],
                attacker["tx_history_count"],
                attacker["stake_amount"]
            )

            # Calculate secure fees
            gas_calc = self.calculate_secure_gas_pricing(
                attacker["content_length"],
                attacker["difficulty"],
                sybil_measures["reputation"],
                attacker["stake_amount"]
            )

            # Calculate rate limiting
            rate_limit = self.calculate_rate_limiting(
                sybil_measures["reputation"],
                int(time.time()) - 3600  # Assume last submission 1 hour ago
            )

            # Determine protection effect
            protection_level = "❌ High risk"
            if sybil_measures["reputation"] < 0.1:
                protection_level = "🚫 Access denied"
            elif sybil_measures["reputation"] < 0.3:
                protection_level = "⚠️ Strict restrictions"
            elif sybil_measures["reputation"] < 0.7:
                protection_level = "✅ Basic protection"
            else:
                protection_level = "🎯 Trusted user"

            print(f"{attacker['name']:<20} {sybil_measures['reputation']:<8.2f} {'/'.join(sybil_measures['access_levels']):<15} {gas_calc['final_cost']:<15,} {rate_limit['cooldown_period']//60:<12}min {protection_level:<20}")

        print("\n" + "="*80)
        print("🛡️ Security Measures Details")
        print("="*80)

        print("💰 **1. Economic Threshold Protection**")
        print("   - Minimum fee: 50,000 nanoTOS (0.00005 TOS)")
        print("   - Stake multiplier: Low-stake users pay 2-5x fees")
        print("   - Reputation discount: High-reputation users get 30-50% fee reduction")

        print("\n🔐 **2. Anti-Sybil Mechanism**")
        print("   - Account age: 30 days to max score (30% weight)")
        print("   - Transaction history: 100 transactions to max score (40% weight)")
        print("   - Stake amount: 0.001 TOS to max score (30% weight)")

        print("\n⏱️ **3. Rate Limiting**")
        print("   - High reputation (0.8+): 5-minute cooldown")
        print("   - Medium reputation (0.5-0.8): 15-minute cooldown")
        print("   - Low reputation (0.3-0.5): 30-minute cooldown")
        print("   - Very low reputation (<0.3): 1-hour cooldown")

        print("\n🎯 **4. Access Control**")
        print("   - Basic tasks: 0.1+ reputation")
        print("   - Intermediate tasks: 0.3+ reputation")
        print("   - Advanced tasks: 0.5+ reputation")
        print("   - Expert tasks: 0.7+ reputation")

        print("\n📊 **5. Progressive Pricing**")
        print("   - 0-50 bytes: Minimum fee 50,000 nanoTOS")
        print("   - 50-200 bytes: 0.0000005 TOS/byte")
        print("   - 200-1000 bytes: 0.000001 TOS/byte")
        print("   - 1000+ bytes: 0.000002 TOS/byte")

        print("\n" + "="*80)
        print("✅ Security Assessment Results")
        print("="*80)

        print("🎯 **Attack Cost Analysis**:")
        print("   - Spam attacks: 5x fee increase + long cooldown times")
        print("   - Sybil attacks: Require long-term account building + large stakes")
        print("   - Bulk attacks: Rate limiting + reputation penalties")

        print("\n💡 **Normal User Experience**:")
        print("   - New users: Reasonable thresholds, gradually build reputation")
        print("   - Veteran users: Fee discounts, faster operation frequency")
        print("   - Quality users: Maximum benefits, highest privileges")

        print("\n🔄 **Dynamic Adjustment Mechanisms**:")
        print("   - Adjust minimum fees based on network congestion")
        print("   - Update protection parameters based on attack patterns")
        print("   - Optimize reputation algorithms based on user feedback")

        print("\n✅ **Summary**: Through multi-layer protection mechanisms, effectively prevent various attacks")
        print("     while maintaining friendly experience and economic incentives for normal users!")

def main():
    """Main function"""
    demo = SecureTOSAIMiningDemo()
    demo.run_security_analysis()

if __name__ == "__main__":
    main()